#!/usr/bin/env bash
# The `secrets` stage (FND-04, FND-08, FND-09, fnd §7.4): gitleaks over the full history of a
# repository with the default ruleset and no path allowlisted. The policy is .gitleaks.toml, which
# must be exactly what scripts/gitleaks-config.sh derives from the gitleaks on PATH: its default
# configuration without the global [allowlist] that skips images, PDFs, node_modules/, go.sum,
# package-lock.json and other lockfiles (rules keep their own value allowlists of placeholders).
# One pass, `gitleaks dir`, over every blob reachable from a branch, a tag, a remote-tracking
# ref or HEAD (what is or can be pushed), each written out whole as text, and over every commit
# and annotated tag message. Not `gitleaks git`: `git log -p` shows nothing of a file git takes
# for binary (a NUL byte, a `-diff` or `binary` attribute) and nothing a merge resolution added
# (the FND-08 re-verification's F2), and gitleaks itself skips a file it sniffs as binary (an
# `MZ` or `%PDF-` header; the FND-09 review's G1), so the pass fails on any skipped file and on
# any archive. Every `.gitattributes` in the history may hold only allowlisted attributes
# (`text`, `text=auto`, `eol=lf`, `eol=crlf` and `linguist-*`). The only blobs not scanned are
# `reviewed_blobs` below, by exact object id, each with its reason: history that cannot change.
# The report is redacted: it names the rule, `<blob>/<path>` and the line, then the commits that
# hold the blob, never the secret.
#
# Usage: scripts/secrets-check.sh [--config FILE] [REPO]    (default: this repository)
# Exit: 0 no finding, 1 a finding, a shallow history or a policy that allowlists something,
# 2 usage. A shallow clone scans only part of the history, so it fails closed: CI fetches full
# history. The working tree is not scanned until it is committed; the next gate then sees it.
#
# Pinned by: scripts/test-secrets.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
config="$root/.gitleaks.toml"
repo="$root"
while [ $# -gt 0 ]; do
  case "$1" in
    --config) [ $# -ge 2 ] || { echo "secrets: --config needs a path" >&2; exit 2; }; config="$2"; shift ;;
    -h | --help) sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//' >&2; exit 2 ;;
    -*) echo "secrets: unknown option $1" >&2; exit 2 ;;
    *) repo="$1" ;;
  esac
  shift
done

command -v gitleaks >/dev/null 2>&1 || {
  echo "gitleaks is missing: run 'mise install --locked' (or bash scripts/bootstrap-tools.sh); just doctor says what else is" >&2
  exit 1
}
[ -f "$config" ] || { echo "secrets: $config is missing" >&2; exit 1; }
# gitleaks resolves a relative --config against the scanned repository, which may hold its own.
config="$(cd "$(dirname "$config")" && pwd -P)/$(basename "$config")"
# The policy is pinned by its shape, not checked for bad keys: byte for byte the generated one, so
# no edit can disable a rule, add an allowlist or redefine one.
bash "$root/scripts/gitleaks-config.sh" --check "$config" || {
  echo "secrets: $config must be exactly the generated policy; it allows no path and disables nothing (fnd §7.4)" >&2
  exit 1
}
# The other two ways to wave a finding through are refused as well: a committed .gitleaksignore
# (its fingerprints) and `gitleaks:allow` comments in the scanned text.
if git -C "$repo" log --all --name-only --format= -- '.gitleaksignore' '**/.gitleaksignore' | grep -q .; then
  echo "secrets: a .gitleaksignore exists in the history of $repo; the policy allows nothing (fnd §7.4)" >&2
  exit 1
fi
if [ -e "$repo/.gitleaksignore" ]; then
  echo "secrets: $repo/.gitleaksignore exists (tracked or not); the policy allows nothing (fnd §7.4)" >&2
  exit 1
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
empty="$work/empty"
: >"$empty"
if [ "$(git -C "$repo" rev-parse --is-shallow-repository)" = "true" ]; then
  echo "secrets: $repo is a shallow clone, so the history is not all there; fetch it in full" >&2
  exit 1
fi

# Every blob reachable from any ref, with each path it has had: `<sha> <path>` lines.
# What is or can be pushed: branches, tags, remote-tracking refs and HEAD (another worktree's
# detached HEAD or a scratch ref is not).
scope=(--branches --tags --remotes HEAD)
# Blobs of the history that are not scanned, by exact object id, each with its reason. Only a
# blob already on main that cannot change belongs here (FND-09).
reviewed_blobs="$(cat <<'LIST'
af3c0c925492b1e9241c4a1610e2de21f9205945 .gitleaks.toml of 2f9306d (FND-08): gitleaks 8.30.1's default policy, whose own rule texts and public test keys match its rules; its successor holds no such literal
LIST
)"
git -C "$repo" rev-list --objects "${scope[@]}" |
  git -C "$repo" cat-file --batch-check='%(objecttype) %(objectname) %(rest)' |
  sed -n 's/^blob \([0-9a-f]*\) \(.*\)$/\1 \2/p' >"$work/blobs"

# The attributes allowlist: every line of every .gitattributes ever committed is a comment, a
# blank, or a pattern followed by allowlisted attributes only (no macro definitions).
status=0
while read -r sha path; do
  case "/$path" in */.gitattributes) ;; *) continue ;; esac
  if ! line="$(git -C "$repo" cat-file blob "$sha" | awk '
      /^[[:space:]]*(#.*)?$/ { next }
      /^\[attr\]/ { print NR ": " $0; bad = 1; exit }
      {
        for (i = 2; i <= NF; i++) {
          if ($i !~ /^(text|text=auto|eol=lf|eol=crlf|linguist-[a-z]+(=[A-Za-z0-9_+-]+)?|-?linguist-[a-z]+)$/) {
            print NR ": " $0; bad = 1; exit
          }
        }
        if (NF < 2) { print NR ": " $0; bad = 1; exit }
      }
      END { exit bad }')"; then
    echo "secrets: $path (blob $sha) line $line: an attribute outside the allowlist (text, text=auto, eol=lf, eol=crlf, linguist-*); binary, -diff, -text, diff=, filter= and the like change what git shows of a file (fnd §7.4)" >&2
    status=1
  fi
done <"$work/blobs"
[ "$status" -eq 0 ] || exit 1

# Run from the repository root so mise's shims resolve the pinned gitleaks.
cd "$root"

# Each distinct blob once, as the file <sha>/<its first path>; a path component that could leave
# the directory (`.`, `..`, empty) is renamed. gitleaks sniffs a file's type and silently skips
# one it takes for binary (an `MZ`, `%PDF-`, ELF, zip or tar header before plain text; the FND-09
# review's G1), so every blob is written as text: a leading newline, then every byte outside tab,
# newline and printable ASCII turned into a newline (a token is printable ASCII, so it survives
# whole). A blob in one of the common archive and compression formats listed below (zip, gzip,
# xz, bzip2, zstd, 7z, rar, tar, by magic number) would still be unread inside, so one fails the
# stage: the repository commits none. Other encodings (zlib, raw deflate, lz4, brotli, base64
# beyond gitleaks' own decoding, encryption) are out of scope by design (fnd §7.4).
mkdir -p "$work/tree/_messages"
printf '%s\n' "$reviewed_blobs" | awk '{ print $1 }' >"$work/reviewed"
awk 'NR == FNR { skip[$1] = 1; next } !seen[$1]++ && !skip[$1] { print $1 }' \
  "$work/reviewed" "$work/blobs" >"$work/shas"
git -C "$repo" cat-file --batch <"$work/shas" | perl -e '
  use strict; use warnings;
  my ($list, $dir, $refused) = @ARGV;
  open(my $l, "<", $list) or die "$list: $!";
  open(my $r, ">", $refused) or die "$refused: $!";
  my %path;
  while (my $line = <$l>) {
    chomp $line;
    my ($sha, $p) = split / /, $line, 2;
    $path{$sha} //= $p;
  }
  binmode STDIN;
  while (my $header = <STDIN>) {
    my ($sha, $type, $size) = split / /, $header;
    die "unexpected object $header" unless defined $size && $type eq "blob";
    my $read = 0; my $data = "";
    while ($read < $size) {
      my $n = read(STDIN, $data, $size - $read, $read) or die "short read of $sha";
      $read += $n;
    }
    read(STDIN, my $nl, 1);
    if ($data =~ /\A(?:\x1f\x8b|PK[\x03\x05\x07][\x04\x06\x08]|\xfd7zXZ\x00|BZh[1-9]1AY&SY|\x28\xb5\x2f\xfd|7z\xbc\xaf\x27\x1c|Rar!\x1a\x07)/
        || (length($data) > 262 && substr($data, 257, 5) eq "ustar")) {
      print $r "$sha $path{$sha}\n";
    }
    $data =~ tr/\t\n\x20-\x7e/\n/c;
    my @parts = map { ($_ eq "" || $_ eq "." || $_ eq "..") ? "_" : $_ } split m{/}, $path{$sha}, -1;
    my $file = join "/", $dir, $sha, @parts;
    my $parent = $file; $parent =~ s{/[^/]*$}{};
    system("mkdir", "-p", "--", $parent) == 0 or die "mkdir $parent";
    open(my $out, ">:raw", $file) or die "$file: $!";
    print $out "\n", $data;
    close $out or die "$file: $!";
  }
' "$work/blobs" "$work/tree" "$work/archives"
while read -r sha path; do
  echo "secrets: $path (blob $sha) is an archive or compressed file, whose content no pass reads; the repository commits none (fnd §7.4)" >&2
  status=1
done <"$work/archives"
# Commit and tag objects are pushed and public too (G6, H4): every commit object whole (its
# headers, author, committer, signature and message), and every annotated tag object, as text
# files of their own.
git -C "$repo" rev-list "${scope[@]}" | git -C "$repo" cat-file --batch |
  LC_ALL=C tr -c '\11\12\40-\176' '\n' >"$work/tree/_messages/commits"
git -C "$repo" for-each-ref --format='%(objecttype) %(objectname) %(refname)' refs/tags |
  while read -r type sha ref; do
    [ "$type" = tag ] || continue
    printf 'tag %s\n' "$ref"
    git -C "$repo" cat-file tag "$sha"
  done | LC_ALL=C tr -c '\11\12\40-\176' '\n' >"$work/tree/_messages/tags"
# The scan fails closed: any file gitleaks skips as binary is a finding of its own.
echo "secrets: gitleaks over every blob of the history and every message, as text ($(wc -l <"$work/shas" | tr -d ' ') blobs)" >&2
(cd "$work/tree" && gitleaks dir --no-banner --redact --verbose --log-level debug --ignore-gitleaks-allow \
  --gitleaks-ignore-path "$empty" --config "$config" .) >"$work/dir.log" 2>&1 || status=1
grep -v ' DBG ' "$work/dir.log" >&2 || true
# Every path and commit of each blob with a finding.
sed -n 's/^File:[[:space:]]*\([0-9a-f]\{40,64\}\)\/.*$/\1/p' "$work/dir.log" | sort -u |
  while read -r sha; do
    echo "secrets: blob $sha, the commits that add or remove it and its paths there:" >&2
    git -C "$repo" log "${scope[@]}" --format='  commit %h' --name-only --find-object="$sha" | sed '/^$/d' >&2
  done
# gitleaks' debug lines read `<time> DBG skipping <kind> ...`; every skip but an empty file fails.
if perl -ne 's/\e\[[0-9;]*m//g; if (/^\S+\s+DBG\s+skipping\s/ && !/^\S+\s+DBG\s+skipping empty file(\s|$)/) { print STDERR; $bad = 1 } END { exit($bad ? 0 : 1) }' \
    "$work/dir.log"; then
  echo "secrets: gitleaks skipped a file of the history (above), so it was not read; the stage fails closed (fnd §7.4)" >&2
  status=1
fi
exit "$status"
