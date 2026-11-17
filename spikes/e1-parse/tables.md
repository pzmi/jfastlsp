### Warm cache, cumulative stages (median of 3; work = after listing; total includes the gitignore-aware walk)

| repo | thr | read ms | +hash ms | +parse ms | +extract ms | total ms | files/s | MB/s | p50 ms | p95 ms | max ms | peak RSS MB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| junit-framework | 1 | 8 | 9 | 508 | 579 | 592 | 3131 | 13.7 | 0.135 | 1.143 | 9.2 | 11 |
| junit-framework | 4 | 2 | 2 | 126 | 147 | 157 | 12368 | 54.0 | 0.134 | 1.178 | 9.1 | 15 |
| kafka | 1 | 26 | 43 | 5030 | 5944 | 5975 | 1061 | 12.5 | 0.323 | 3.289 | 135.9 | 45 |
| kafka | 4 | 9 | 13 | 1299 | 1555 | 1594 | 4056 | 47.6 | 0.320 | 3.378 | 137.7 | 84 |
| spring-framework | 1 | 42 | 53 | 3444 | 3954 | 4001 | 2350 | 13.6 | 0.196 | 1.512 | 37.2 | 35 |
| spring-framework | 4 | 14 | 16 | 877 | 997 | 1042 | 9326 | 53.9 | 0.198 | 1.535 | 40.4 | 46 |
| gradle | 1 | 41 | 57 | 2210 | 2460 | 2685 | 4774 | 15.4 | 0.086 | 0.798 | 8.3 | 30 |
| gradle | 4 | 14 | 19 | 580 | 660 | 909 | 17788 | 57.3 | 0.087 | 0.801 | 8.4 | 33 |
| elasticsearch | 1 | 134 | 212 | 23940 | 26925 | 27149 | 1257 | 12.4 | 0.389 | 2.684 | 57.8 | 138 |
| elasticsearch | 4 | 41 | 65 | 5993 | 6975 | 7192 | 4853 | 47.9 | 0.394 | 2.737 | 63.1 | 153 |

### Cold cache (echo 3 > drop_caches before each run; single run)

| repo | thr | list ms | read-only work ms | read MB/s | full extract work ms | full total ms | files/s | MB/s | peak RSS MB |
|---|---|---|---|---|---|---|---|---|---|
| junit-framework | 1 | 99 | 94 | 83.9 | 777 | 876 | 2333 | 10.2 | 11 |
| junit-framework | 4 | 54 | 30 | 262.3 | 193 | 246 | 9417 | 41.1 | 15 |
| kafka | 1 | 142 | 324 | 228.3 | 6626 | 6768 | 952 | 11.2 | 44 |
| kafka | 4 | 109 | 131 | 563.0 | 1652 | 1761 | 3817 | 44.8 | 71 |
| spring-framework | 1 | 169 | 450 | 119.5 | 4866 | 5036 | 1910 | 11.0 | 34 |
| spring-framework | 4 | 191 | 180 | 298.3 | 1232 | 1423 | 7543 | 43.6 | 47 |
| gradle | 1 | 1026 | 605 | 62.5 | 3389 | 4415 | 3465 | 11.2 | 30 |
| gradle | 4 | 1128 | 201 | 187.8 | 885 | 2014 | 13266 | 42.7 | 33 |
| elasticsearch | 1 | 829 | 1675 | 199.4 | 28933 | 29762 | 1170 | 11.5 | 138 |
| elasticsearch | 4 | 954 | 601 | 555.4 | 7740 | 8694 | 4373 | 43.1 | 156 |

### Parse-error rate (files whose tree has ERROR or MISSING), warm, 4 threads

| repo | files | decls+imports extracted | orchard err files | upstream (tree-sitter-java 0.23.5) err files | orchard extract ms | upstream extract ms |
|---|---|---|---|---|---|---|
| junit-framework | 1813 | 42198 | 0 (0.00%) | 6 (0.33%) | 148 | 157 |
| kafka | 6307 | 225048 | 3 (0.05%) | 3 (0.05%) | 1593 | 1526 |
| spring-framework | 9294 | 237869 | 0 (0.00%) | 58 (0.62%) | 983 | 1003 |
| gradle | 11743 | 204414 | 11 (0.09%) | 12 (0.10%) | 641 | 628 |
| elasticsearch | 33850 | 1203767 | 0 (0.00%) | 16 (0.05%) | 7022 | 6881 |
