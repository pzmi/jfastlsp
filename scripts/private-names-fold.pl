# The ASCII folding shared by scripts/private-name-add.sh and scripts/private-names-check.sh
# (fnd §7.4): both sides must fold identically or a listed name stops matching. Decodes UTF-8
# (an invalid byte becomes a separator, the rest of the line still folds), applies NFKD, drops combining marks, maps the letters NFKD leaves alone
# (ł ø đ ß æ œ ð þ ı ħ) and turns any other non-ASCII character into a space.
# Pinned by: scripts/test-private-names.sh
use strict; use warnings;
use Unicode::Normalize qw(NFKD);
use Encode qw(decode FB_DEFAULT);
my %map = ("\x{142}" => 'l', "\x{141}" => 'L', "\x{f8}" => 'o', "\x{d8}" => 'O', "\x{111}" => 'd',
  "\x{110}" => 'D', "\x{df}" => 'ss', "\x{e6}" => 'ae', "\x{c6}" => 'AE', "\x{153}" => 'oe',
  "\x{152}" => 'OE', "\x{f0}" => 'd', "\x{d0}" => 'D', "\x{fe}" => 'th', "\x{de}" => 'Th',
  "\x{131}" => 'i', "\x{127}" => 'h', "\x{126}" => 'H');
sub fold_ascii {
  my $b = shift;
  my $t = decode('UTF-8', $b, FB_DEFAULT);
  $t = NFKD($t);
  $t =~ s/\p{M}+//g;
  $t =~ s/([^\x00-\x7f])/exists $map{$1} ? $map{$1} : ' '/ge;
  return $t;
}
1;
