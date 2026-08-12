#!/usr/bin/env python3
"""Independent R4.2.c patch-authority verifier."""
import argparse, copy, hashlib, json, os, re, shutil, subprocess, tempfile
from pathlib import Path

class Error(RuntimeError): pass
def fail(s): raise Error(s)
def pairs(xs):
 d={}
 for k,v in xs:
  if k in d: fail(f"duplicate JSON key: {k}")
  d[k]=v
 return d
def load(p):
 try: v=json.loads(p.read_text(), object_pairs_hook=pairs)
 except (OSError,json.JSONDecodeError) as e: fail(f"cannot read {p}: {e}")
 if not isinstance(v,dict): fail(f"root is not object: {p}")
 return v
def ident(v):
 d=copy.deepcopy(v); d.pop("contentIdentitySha256",None)
 return hashlib.sha256(json.dumps(d,sort_keys=True,separators=(",",":")).encode()).hexdigest()
FIELDS={"artifact","auditDate","bindings","compatibilityOracle","contentIdentitySha256","decisionInventory","independentVerifier","kind","nonclaims","productionEntrypoints","releaseGraphDisposition","reportKinds","runtimeEvidence","schema","sourceModules","spec","version"}
def validate(p,s,check_id=True):
 if s.get("type")!="object" or s.get("additionalProperties") is not False or set(s.get("required",[]))!=FIELDS or set(s.get("properties",{}))!=FIELDS: fail("schema closure drift")
 if set(p)!=FIELDS: fail("profile field drift")
 constants={"artifact":"selfhost/toolchain.gc","independentVerifier":"scripts/lib/selfhost_patch_authority.py","kind":"genesis/selfhost-patch-authority-v0.1","productionEntrypoints":["genesis","genesis_wasi"],"schema":"docs/spec/SELFHOST_PATCH_AUTHORITY_v0.1.schema.json","spec":"docs/spec/SELFHOST_PATCH_AUTHORITY_v0.1.md","version":"0.1.0"}
 for k,v in constants.items():
  if p[k]!=v: fail(f"profile {k} drift")
 if check_id and p["contentIdentitySha256"]!=ident(p): fail("profile content identity mismatch")
 for k,n in (("bindings",7),("decisionInventory",10),("sourceModules",10)):
  if not isinstance(p[k],list) or len(p[k])<n or len(p[k])!=len(set(p[k])): fail(f"{k} incomplete or duplicated")
 if len(p["bindings"])!=7 or len(p["decisionInventory"])!=12 or len(p["sourceModules"])!=12: fail("exact authority inventory cardinality drift")
 if p["bindings"]!=sorted(p["bindings"]) or p["decisionInventory"]!=sorted(p["decisionInventory"]): fail("authority inventories must be sorted")
 if p["compatibilityOracle"]!={"crate":"gc_patches","feature":"parity-oracle","package":"gc_cli_driver_parity","sunsetReviewDate":"2026-11-11"}: fail("oracle custody drift")
 if p["releaseGraphDisposition"]!={"h2Eligible":False,"reason":"parity-feature-unified-in-outer-cli-packages","requiredOwnerPaths":["crates/gc_cli/Cargo.toml","crates/gc_wasi_cli/Cargo.toml"]}: fail("release graph blocker drift")
 if p["runtimeEvidence"]!={"allocationLimit":50000000,"fixture":"tests/spec/pkg_basic","lowStepControl":1,"stepLimit":50000000,"timeoutSeconds":60}: fail("runtime evidence drift")
def tree(root,pkg):
 r=subprocess.run(["cargo","tree","-p",pkg,"-e","features","--locked","--offline"],cwd=root,text=True,capture_output=True)
 if r.returncode: fail(f"cargo tree failed for {pkg}: {r.stderr.strip()}")
 return r.stdout
def static(root,p):
 m=(root/"selfhost/toolchain_manifest.gc").read_text(); mods=re.findall(r'"(selfhost/patch_[^"\n]+\.gc)"',m)
 if mods!=p["sourceModules"]: fail("patch source inventory differs from manifest")
 mx=0
 for rel in mods:
  f=(root/rel).resolve()
  if root not in f.parents or f.is_symlink() or not f.is_file(): fail(f"invalid source path: {rel}")
  mx=max(mx,len(f.read_text().splitlines()))
 for b in p["bindings"]:
  if b not in m: fail(f"missing binding: {b}")
 src="\n".join(x.read_text() for x in (root/"crates/gc_patches/src").glob("*.rs"))
 for tok in ("Rust patch parser is not compiled into production","Rust patch identity oracle is not compiled into production","patch apply requires artifact-only GenesisCode semantic and report authority","patch-apply-report"):
  if tok not in src: fail(f"missing fail-closed boundary: {tok}")
 if "fn report_term(" in src or "genesis/patch-apply-v0.2" in src: fail("retired Rust report producer remains")
 if 'gc_patches feature "parity-oracle"' in tree(root,"gc_cli_driver"): fail("normal driver activates patch oracle")
 parity_manifest=(root/"crates/gc_cli_driver_parity/Cargo.toml").read_text()
 if 'parity-harness = ["gc_obligations/parity-oracle", "gc_patches/parity-oracle"]' not in parity_manifest: fail("parity package lost patch oracle custody")
 for pkg in ("gc_cli","gc_wasi_cli"):
  if 'gc_patches feature "parity-oracle"' not in tree(root,pkg): fail("declared package blocker no longer factual")
 mains=(root/"crates/gc_cli/src/main.rs").read_text()+(root/"crates/gc_wasi_cli/src/main.rs").read_text()
 if "gc_cli_driver_parity" in mains or mains.count("gc_cli_driver::run")!=2: fail("production dispatch drift")
 schema=(root/"crates/gc_cli_driver/src/cli_schema.rs").read_text()
 if 'RuntimeProfile::Production => vec!["selfhost".to_string()]' not in schema: fail("production exposes non-selfhost frontend")
 return {"moduleCount":len(mods),"maxSourceLines":mx,"h2Eligible":False}
def command(bin,artifact,patch,pkg,p,steps): return [str(bin),"--json","--selfhost-only","--selfhost-artifact",str(artifact),"--coreform-frontend","selfhost","--step-limit",str(steps),"--max-alloc-units",str(p["runtimeEvidence"]["allocationLimit"]),"apply-patch",str(patch),"--pkg",str(pkg)]
def runtime(root,p,bins):
 art=(root/p["artifact"]).resolve(); fixture=(root/p["runtimeEvidence"]["fixture"]).resolve(); obs=[]
 with tempfile.TemporaryDirectory(prefix="genesis-patch-authority-") as t:
  t=Path(t)
  for bin in bins:
   bin=bin.resolve()
   if not bin.is_file() or not os.access(bin,os.X_OK): fail(f"not executable: {bin}")
   w=t/bin.name; shutil.copytree(fixture,w)
   r=subprocess.run(command(bin,art,w/"pure.gcpatch",w/"package.toml",p,50000000),cwd=root,capture_output=True,timeout=60)
   try: e=json.loads(r.stdout)
   except Exception: fail(f"{bin.name} invalid JSON: {r.stderr.decode(errors='replace')}")
   if r.returncode or e.get("ok") is not True: fail(f"{bin.name} valid apply failed")
   a={k:e.get("data",{}).get(k) for k in ("patch_artifact","report_artifact","acceptance_artifact","package_artifact")}
   if not all(isinstance(v,str) and re.fullmatch(r"[0-9a-f]{64}",v) for v in a.values()): fail(f"{bin.name} invalid identities")
   lw=t/(bin.name+"-low"); shutil.copytree(fixture,lw); before=(lw/"basic.gc").read_bytes()
   q=subprocess.run(command(bin,art,lw/"pure.gcpatch",lw/"package.toml",p,1),cwd=root,capture_output=True,timeout=60)
   try: qe=json.loads(q.stdout)
   except Exception: fail(f"{bin.name} low-step invalid JSON")
   if q.returncode==0 or qe.get("ok") is not False or (lw/"basic.gc").read_bytes()!=before: fail(f"{bin.name} low-step not fail-closed")
   obs.append({"entrypoint":bin.name,"artifacts":a,"lowStepExit":q.returncode})
 if obs[0]["artifacts"]!=obs[1]["artifacts"] or obs[0]["lowStepExit"]!=obs[1]["lowStepExit"]: fail("native/WASI divergence")
 return obs
def mutations(p,s):
 edits=[("binding",lambda x:x["bindings"].pop()),("decision",lambda x:x["decisionInventory"].pop()),("source",lambda x:x["sourceModules"].pop()),("entrypoint",lambda x:x.__setitem__("productionEntrypoints",["genesis_parity"])),("oracle",lambda x:x["compatibilityOracle"].__setitem__("feature","default")),("promotion",lambda x:x["releaseGraphDisposition"].__setitem__("h2Eligible",True)),("runtime",lambda x:x["runtimeEvidence"].__setitem__("lowStepControl",50000000)),("unknown",lambda x:x.__setitem__("unexpected",True))]
 for label,edit in edits:
  c=copy.deepcopy(p); edit(c); c["contentIdentitySha256"]=ident(c)
  try: validate(c,s)
  except Error: continue
  fail(f"self-test accepted authority mutation: {label}")
 c=copy.deepcopy(p); c["auditDate"]="2026-08-12"
 try: validate(c,s)
 except Error: return len(edits)+1
 fail("self-test accepted stale identity")
def main():
 a=argparse.ArgumentParser(); a.add_argument("--root",type=Path,default=Path.cwd()); a.add_argument("--profile",type=Path,required=True); a.add_argument("--schema",type=Path,required=True); a.add_argument("--refresh-identity",action="store_true"); a.add_argument("--self-test",action="store_true"); a.add_argument("--runtime",action="store_true"); a.add_argument("--genesis-bin",type=Path); a.add_argument("--genesis-wasi-bin",type=Path); x=a.parse_args(); root=x.root.resolve(); pp=root/x.profile; sp=root/x.schema; p=load(pp); s=load(sp)
 if x.refresh_identity:
  p["contentIdentitySha256"]=ident(p); pp.write_text(json.dumps(p,indent=2)+"\n"); print(f"selfhost-patch-authority: refreshed {x.profile}"); return
 validate(p,s); st=static(root,p); controls=mutations(p,s) if x.self_test else 0; rt=None
 if x.runtime:
  if not x.genesis_bin or not x.genesis_wasi_bin: fail("runtime binaries required")
  rt=runtime(root,p,[x.genesis_bin,x.genesis_wasi_bin])
 print(json.dumps({"kind":"genesis/selfhost-patch-authority-check-v0.1","ok":True,"profileIdentitySha256":ident(p),"static":st,"mutationControls":controls,"runtime":rt},sort_keys=True,separators=(",",":")))
if __name__=="__main__":
 try: main()
 except Error as e: print(f"selfhost-patch-authority: {e}",file=os.sys.stderr); raise SystemExit(1)
