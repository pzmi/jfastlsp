#!/usr/bin/env bash
# Floors: rg count, git ls-files, git status; cold (dropped caches) and warm. Wall seconds via GNU time.
set -u
D=$(cd "$(dirname "$0")" && pwd)
t() { /usr/bin/time -f '%e %M' "$@" 2>&1 >/dev/null | tail -1; }
for r in ${REPOS:-junit-framework kafka spring-framework gradle elasticsearch}; do
  cd $D/repos/$r
  sync; echo 3 > /proc/sys/vm/drop_caches; rgc=$(t rg -c -t java '\bString\b' .)
  rgw1=$(t rg -c -t java '\bString\b' .); rgw=$(t rg -c -t java '\bString\b' .)
  rg1=$(t rg -j1 -c -t java '\bString\b' .)
  sync; echo 3 > /proc/sys/vm/drop_caches; stc=$(t git status --porcelain)
  stw=$(t git status --porcelain); stw=$(t git status --porcelain)
  lsw=$(t sh -c "git ls-files '*.java' | wc -l")
  lss=$(t git ls-files -s)
  echo "FLOOR repo=$r rg_cold=[$rgc] rg_warm=[$rgw] rg_warm_j1=[$rg1] status_cold=[$stc] status_warm=[$stw] lsfiles_java=[$lsw] lsfiles_s=[$lss] matches=$(rg -c -t java '\bString\b' . | awk -F: '{s+=$2} END{print s}')"
done
