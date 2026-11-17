#!/usr/bin/env bash
# E1 spike driver. Each run prints RESULT ... plus rss_kb via GNU time. Appends to results.log.
set -u
D=$(cd "$(dirname "$0")" && pwd); B=$D/parse-spike/target/release/parse-spike; L=$D/results.log
run() { local tag=$1; shift; out=$(/usr/bin/time -f 'rss_kb=%M' "$B" "$@" 2>&1 | tr '\n' ' '); echo "$tag repo=$(basename $1) $out" | tee -a "$L"; }
for r in ${REPOS:-junit-framework kafka spring-framework gradle elasticsearch}; do
  p=$D/repos/$r
  for t in 1 4; do sync; echo 3 > /proc/sys/vm/drop_caches; run COLD "$p" --threads $t --upto read
                   sync; echo 3 > /proc/sys/vm/drop_caches; run COLD "$p" --threads $t --upto extract; done
  run WARMUP "$p" --threads 4 --upto read
  for t in 1 4; do for s in read hash parse extract; do for i in 1 2 3; do run WARM "$p" --threads $t --upto $s; done; done; done
  run WARM "$p" --threads 4 --upto extract --grammar upstream --errors-out $D/errors-upstream-$r.txt
  run WARM "$p" --threads 4 --upto extract --grammar orchard --errors-out $D/errors-orchard-$r.txt
done
