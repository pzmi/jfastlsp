# Turns results.log into Markdown tables (median of the three warm repetitions).
import re, statistics as st, sys
rows=[]
for line in open(sys.argv[1]):
    tag=line.split()[0]; d=dict(re.findall(r'(\w+)=([^\s]+)',line)); d['tag']=tag; rows.append(d)
repos=['junit-framework','kafka','spring-framework','gradle','elasticsearch']
def sel(**k): return [r for r in rows if all(r.get(a)==str(b) for a,b in k.items())]
def med(rs,f): return st.median(float(r[f]) for r in rs)
print('### Warm cache, cumulative stages (median of 3; work = after listing; total includes the gitignore-aware walk)\n')
print('| repo | thr | read ms | +hash ms | +parse ms | +extract ms | total ms | files/s | MB/s | p50 ms | p95 ms | max ms | peak RSS MB |')
print('|---|---|---|---|---|---|---|---|---|---|---|---|---|')
for r in repos:
  for t in (1,4):
    w={s:sel(tag='WARM',repo=r,threads=t,upto=s,grammar='orchard')[:3] for s in ('read','hash','parse','extract')}
    e=w['extract']
    print(f"| {r} | {t} | {med(w['read'],'work_ms'):.0f} | {med(w['hash'],'work_ms'):.0f} | {med(w['parse'],'work_ms'):.0f} | {med(e,'work_ms'):.0f} | {med(e,'total_ms'):.0f} | {med(e,'files_per_s'):.0f} | {med(e,'mb_per_s'):.1f} | {med(e,'p50_ms'):.3f} | {med(e,'p95_ms'):.3f} | {med(e,'max_ms'):.1f} | {med(e,'rss_kb')/1024:.0f} |")
print('\n### Cold cache (echo 3 > drop_caches before each run; single run)\n')
print('| repo | thr | list ms | read-only work ms | read MB/s | full extract work ms | full total ms | files/s | MB/s | peak RSS MB |')
print('|---|---|---|---|---|---|---|---|---|---|')
for r in repos:
  for t in (1,4):
    a=sel(tag='COLD',repo=r,threads=t,upto='read')[0]; b=sel(tag='COLD',repo=r,threads=t,upto='extract')[0]
    print(f"| {r} | {t} | {b['list_ms']} | {a['work_ms']} | {a['mb_per_s']} | {b['work_ms']} | {b['total_ms']} | {b['files_per_s']} | {b['mb_per_s']} | {float(b['rss_kb'])/1024:.0f} |")
print('\n### Parse-error rate (files whose tree has ERROR or MISSING), warm, 4 threads\n')
print('| repo | files | decls+imports extracted | orchard err files | upstream (tree-sitter-java 0.23.5) err files | orchard extract ms | upstream extract ms |')
print('|---|---|---|---|---|---|---|')
for r in repos:
  o=sel(tag='WARM',repo=r,threads=4,upto='extract',grammar='orchard')[-1]; u=sel(tag='WARM',repo=r,threads=4,upto='extract',grammar='upstream')[0]
  print(f"| {r} | {o['files']} | {o['decls']} | {o['err_files']} ({o['err_pct']}%) | {u['err_files']} ({u['err_pct']}%) | {o['work_ms']} | {u['work_ms']} |")
