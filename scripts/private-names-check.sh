#!/usr/bin/env bash
# The hashed private-name check (FND-04, fnd §7.4, ADR 0004): fail when text this change adds
# contains a name whose salted hash is listed in .private-names.sha256. The names themselves are
# never in the repository and never printed: a finding names the path and line (or the commit and
# message line) only.
#
# Scope, with BASE = `git merge-base HEAD origin/main` (the diff base of fnd §9, the same lines for
# a lane's uncommitted tree, its commit and land.sh's cherry-picked tree):
#   * the lines added since BASE, committed or not (`git diff BASE`), in tracked files;
#   * every untracked file that is not ignored;
#   * the message, author and committer of each commit in BASE..HEAD, and the branch name;
#   * the names of the added and untracked files (a match masks the path in the report).
# A change that removes an entry from .private-names.sha256, or alters its salt, fails.
# With no `origin/main` (a fork or a shallow checkout), or with --all, the scope is every tracked
# and untracked file plus every commit message of HEAD's history: a superset, never a gap. Under
# CI=true the same superset is scanned when the base is HEAD itself (a push to main, where the
# diff is empty) or, with --base, the all-zero commit (a first push).
#
# Matching: the text is folded to ASCII (NFKD, combining marks dropped, ł ø đ ß æ mapped:
# scripts/private-names-fold.pl, shared with the add script), lower-cased and split on everything
# that is not an ASCII letter or digit,
# and every token and every run of 2 or 3 adjacent tokens, joined, is hashed as
# sha256("<salt>:<candidate>"); a second pass splits camelCase first, so `fooBarService` is also
# read as foo, bar, service. "Foo Bar", "foo-bar" and "FooBar" therefore match one entry, the one
# scripts/private-name-add.sh writes, whatever the accents. It needs no secret. No substring,
# homoglyph or misspelling matching (fnd §7.4).
#
# Usage: scripts/private-names-check.sh [--all] [--hashes FILE] [--base REV]
# Exit: 0 clean (or no hash file outside CI), 1 a match or a missing or empty hash file under
# CI=true, 2 usage. A missing or empty hash file fails closed in CI and in land.sh (which checks
# it itself) and only warns elsewhere: a downstream fork may not have one. A malformed line fails
# everywhere.
#
# Pinned by: scripts/test-private-names.sh
set -euo pipefail

die() { echo "private-names: $*" >&2; exit 2; }

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
hashes="$root/.private-names.sha256"
base=""
all=0
while [ $# -gt 0 ]; do
  case "$1" in
    --all) all=1 ;;
    --hashes | --base)
      [ $# -ge 2 ] || die "$1 needs a value"
      case "$1" in --hashes) hashes="$2" ;; *) base="$2" ;; esac
      shift ;;
    -h | --help) sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//' >&2; exit 2 ;;
    *) die "unknown argument $1" ;;
  esac
  shift
done

command -v perl >/dev/null 2>&1 || { echo "private-names: perl is missing (Digest::SHA hashes the tokens)" >&2; exit 1; }
git rev-parse --show-toplevel >/dev/null 2>&1 || die "not inside a git repository"
cd "$(git rev-parse --show-toplevel)"

if ! grep -qvE '^[[:space:]]*(#|$)' "$hashes" 2>/dev/null; then
  if [ "${CI:-}" = "true" ]; then
    echo "private-names: $hashes is missing or has no entry; the check fails closed under CI=true (fnd §7.4)" >&2
    exit 1
  fi
  echo "private-names: warning: $hashes is missing or has no entry; nothing was checked" >&2
  exit 0
fi

# One perl program for every input kind; it reads git's own output, so line numbers come from the
# diff's hunk headers and nothing is re-parsed in the shell.
read -r -d '' program <<'PERL' || true
use strict; use warnings;
use Digest::SHA qw(sha256_hex);
use Encode ();
my ($mode, $hashfile, $foldlib, $label) = @ARGV;
require $foldlib;
my ($salt, %listed) = ('');
open(my $hf, '<', $hashfile) or die "private-names: cannot read $hashfile\n";
while (my $l = <$hf>) {
  chomp $l; $l =~ s/\r$//;
  if ($l =~ /^# salt: ([0-9a-f]{32,})$/) { $salt = $1; next }
  next if $l =~ /^\s*(#|$)/;
  die "private-names: $hashfile has a line that is not a SHA-256 hex digest\n" unless $l =~ /^[0-9a-f]{64}$/;
  $listed{$l} = 1;
}
die "private-names: $hashfile has no '# salt: <hex>' header\n" if $salt eq '';
my %memo;
sub listed { my $c = shift; return $memo{$c} //= exists $listed{sha256_hex("$salt:$c")} }
sub runs {
  my @t = @_;
  for my $i (0 .. $#t) {
    my $joined = '';
    for my $n (0 .. 2) {
      last if $i + $n > $#t;
      $joined .= $t[$i + $n];
      return 1 if length($joined) >= 3 && listed($joined);
    }
  }
  return 0;
}
sub hit {
  my $line = shift;
  $line =~ s/\0//g;
  $line = fold_ascii($line);
  return 0 unless $line =~ /[A-Za-z0-9]{3}|[A-Za-z0-9]+[^A-Za-z0-9]+[A-Za-z0-9]/;
  my @plain = grep { length } split /[^a-z0-9]+/, lc $line;
  return 1 if runs(@plain);
  my $camel = $line;
  $camel =~ s/([a-z0-9])([A-Z])/$1 $2/g;
  $camel =~ s/([A-Z]+)([A-Z][a-z])/$1 $2/g;
  my @split = grep { length } split /[^a-z0-9]+/, lc $camel;
  return runs(@split);
}
my $found = 0;
sub shown {
  my $p = shift;
  return $p unless hit($p);
  my $s = join('/', map { hit($_) ? '<name>' : $_ } split m{/}, $p);
  return $s eq $p ? '<path withheld>' : $s;
}
sub path_hit { my $p = shift; report('the name of ' . shown($p)) if hit($p) }
sub report { my $where = shift; print STDERR "private-names: $where matches an entry of $hashfile\n"; $found++ }
if ($mode eq 'diff') {
  my ($path, $shownpath, $n, $header) = ('', '', 0, 0);
  while (my $l = <STDIN>) {
    chomp $l;
    if ($l =~ /^diff --git /) { $header = 1; next }
    if ($header) {
      if ($l =~ /^\+\+\+ (?:b\/)?([^\t]*)/) { $path = $1; path_hit($path) if $path ne '/dev/null'; $shownpath = shown($path); next }
      elsif ($l =~ /^@@ /) { $header = 0 } else { next }
    }
    if ($l =~ /^@@ -\S+ \+(\d+)/) { $n = $1; next }
    if ($l =~ /^\+(.*)$/) { report("$shownpath:$n") if hit($1); $n++ }
  }
} elsif ($mode eq 'files') {
  local $/ = "\0";
  while (my $f = <STDIN>) {
    chomp $f;
    next unless -f $f && !-l $f;
    path_hit($f);
    my $sf = shown($f);
    open(my $fh, '<:raw', $f) or next;
    read($fh, my $head, 8192);
    my $utf16 = defined $head && $head =~ /^(?:\xff\xfe|\xfe\xff)/;
    next if !$utf16 && defined $head && index($head, "\0") >= 0;
    seek($fh, 0, 0);
    local $/ = $utf16 ? undef : "\n";
    my $n = 0;
    if ($utf16) {
      my $raw = <$fh>;
      my $text = eval { Encode::decode('UTF-16', $raw, Encode::FB_CROAK) };
      next unless defined $text;
      for my $l (split /\n/, Encode::encode('UTF-8', $text)) { $n++; report("$sf:$n") if hit($l) }
      next;
    }
    while (my $l = <$fh>) { $n++; report("$sf:$n") if hit($l) }
  }
} elsif ($mode eq 'log') {
  my ($commit, $n) = ('', 0);
  while (my $l = <STDIN>) {
    chomp $l;
    if ($l =~ /^\x01([0-9a-f]+)$/) { $commit = $1; $n = 0; next }
    $n++;
    report("commit $commit, message line $n") if hit($l);
  }
} elsif ($mode eq 'lines') {
  my $n = 0;
  while (my $l = <STDIN>) { $n++; report("$label (line $n)") if hit($l) }
} else { die "private-names: unknown mode $mode\n" }
if ($found) {
  print STDERR "private-names: $found match(es); the name is never printed (fnd §7.4). Remove it from the text above; amend the commit message if it is there.\n";
  exit 1;
}
PERL

fold="$root/scripts/private-names-fold.pl"
scan() { perl -e "$program" "$1" "$hashes" "$fold" "${2:-}"; }

rc=0
if [ "$all" -eq 0 ]; then
  if [ -z "$base" ]; then
    base="$(git merge-base HEAD origin/main 2>/dev/null || true)"
    if [ -z "$base" ]; then
      echo "private-names: no origin/main to diff against; scanning every file and every commit message" >&2
      all=1
    fi
  elif [ "${CI:-}" = "true" ] && [[ "$base" =~ ^0+$ ]]; then
    echo "private-names: the base is the all-zero commit (a first push); scanning every file and every commit message" >&2
    all=1
  else
    base="$(git rev-parse --verify --quiet "$base^{commit}")" || die "$base is not a commit"
  fi
  # In CI a push to main has HEAD == origin/main, so the diff would be empty and the check green
  # having read nothing: scan everything instead (fnd §7.4).
  if [ "$all" -eq 0 ] && [ "${CI:-}" = "true" ] && [ "$base" = "$(git rev-parse HEAD)" ]; then
    echo "private-names: the diff base is HEAD under CI=true (a push to main); scanning every file and every commit message" >&2
    all=1
  fi
fi

if [ "$all" -eq 1 ]; then
  { git ls-files -z --cached --others --exclude-standard; } | scan files || rc=1
  git log --format=$'\x01%h%n%B' HEAD | scan log || rc=1
  git log --format='%h %an %ae %cn %ce' HEAD | scan lines "a commit author or committer" || rc=1
else
  # -U0 so every `+` line carries its own hunk header; --no-ext-diff and --no-renames keep the
  # output the plain format whatever the user's git config says.
  git -c core.quotepath=false diff -U0 --text --no-color --no-ext-diff --no-renames --no-textconv "$base" -- | scan diff || rc=1
  git ls-files -z --others --exclude-standard | scan files || rc=1
  git log --format=$'\x01%h%n%B' "$base..HEAD" | scan log || rc=1
  git log --format='%h %an %ae %cn %ce' "$base..HEAD" | scan lines "a commit author or committer" || rc=1
  # An entry removed from the list would silently weaken the check: the lane reads its own copy.
  if [ "$hashes" = "$root/.private-names.sha256" ] && git cat-file -e "$base:.private-names.sha256" 2>/dev/null; then
    removed="$(git show "$base:.private-names.sha256" | grep -vE '^[[:space:]]*($|#)' | grep -vxF -f <(grep -vE '^[[:space:]]*($|#)' "$hashes") || true)"
    if [ -n "$removed" ] || [ "$(git show "$base:.private-names.sha256" | grep '^# salt:')" != "$(grep '^# salt:' "$hashes")" ]; then
      echo "private-names: this change removes an entry from, or changes the salt of, .private-names.sha256; only additions are allowed (fnd §7.4)" >&2
      rc=1
    fi
  fi
fi
branch="$(git branch --show-current 2>/dev/null || true)"
printf '%s\n' "$branch" | scan lines "the current branch name" || rc=1
[ "$rc" -eq 0 ] || exit 1
echo "private-names: no listed name in the checked text" >&2
