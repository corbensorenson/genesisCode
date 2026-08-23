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
 if p["releaseGraphDisposition"]!={"h2Eligible":True,"reason":"parity-feature-isolated-from-production-cli-packages","requiredOwnerPaths":["crates/gc_cli/Cargo.toml","crates/gc_wasi_cli/Cargo.toml"]}: fail("release graph disposition drift")
 if p["runtimeEvidence"]!={"allocationLimit":50000000,"fixture":"tests/spec/pkg_basic","lowStepControl":1,"stepLimit":50000000,"timeoutSeconds":60}: fail("runtime evidence drift")
def tree(root,pkg):
 r=subprocess.run(["cargo","tree","-p",pkg,"-e","features","--locked","--offline"],cwd=root,text=True,capture_output=True)
 if r.returncode: fail(f"cargo tree failed for {pkg}: {r.stderr.strip()}")
 return r.stdout
def metadata(root):
 r=subprocess.run(["cargo","metadata","--no-deps","--format-version","1","--locked","--offline"],cwd=root,text=True,capture_output=True)
 if r.returncode: fail(f"cargo metadata failed: {r.stderr.strip()}")
 try: return json.loads(r.stdout)
 except json.JSONDecodeError as e: fail(f"cargo metadata emitted invalid JSON: {e}")
def mask_rust(source):
 out=list(source); i=0; state="code"; depth=0; raw_hashes=0
 while i<len(source):
  if state=="code":
   raw=re.match(r'(?:br|r)(#+)?"',source[i:]); char=re.match(r"'(?:\\.|[^\\'\n])'",source[i:])
   if source.startswith("//",i): out[i]=out[i+1]=" "; i+=2; state="line"
   elif source.startswith("/*",i): out[i]=out[i+1]=" "; i+=2; state="block"; depth=1
   elif raw:
    token=raw.group(0); raw_hashes=len(raw.group(1) or "")
    for j in range(i,i+len(token)): out[j]=" "
    i+=len(token); state="raw"
   elif char:
    for j in range(i,i+len(char.group(0))): out[j]=" "
    i+=len(char.group(0))
   elif source.startswith('b"',i) or source[i]=='"':
    width=2 if source.startswith('b"',i) else 1
    for j in range(i,i+width): out[j]=" "
    i+=width; state="string"
   else: i+=1
  elif state=="line":
   if source[i]=='\n': state="code"
   else: out[i]=" "
   i+=1
  elif state=="block":
   if source.startswith("/*",i): out[i]=out[i+1]=" "; i+=2; depth+=1
   elif source.startswith("*/",i): out[i]=out[i+1]=" "; i+=2; depth-=1; state="code" if depth==0 else "block"
   else: out[i]=" "; i+=1
  elif state=="string":
   out[i]=" "
   if source[i]=='\\':
    i+=1
    if i<len(source): out[i]=" "; i+=1
   elif source[i]=='"': i+=1; state="code"
   else: i+=1
  else:
   end='"'+'#'*raw_hashes
   if source.startswith(end,i):
    for j in range(i,i+len(end)): out[j]=" "
    i+=len(end); state="code"
   else: out[i]=" "; i+=1
 if state not in ("code","line"): fail("unterminated Rust token while checking parity test custody")
 return "".join(out)
def rust_functions(source):
 clean=mask_rust(source); found=[]
 pattern=re.compile(r'(?m)^[ \t]*(?:pub(?:\([^\n)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b')
 for match in pattern.finditer(clean):
  opening=clean.find("{",match.end())
  if opening<0: fail(f"function lacks body: {match.group(1)}")
  depth=1; end=opening+1
  while end<len(clean) and depth:
   if clean[end]=="{": depth+=1
   elif clean[end]=="}": depth-=1
   end+=1
  if depth: fail(f"unclosed Rust function: {match.group(1)}")
  line=source.rfind("\n",0,match.start())+1; prefix_start=line
  while prefix_start:
   prior_end=prefix_start-1; prior_start=source.rfind("\n",0,prior_end)+1; prior=source[prior_start:prior_end].strip()
   if not prior or prior.startswith("#["): prefix_start=prior_start
   else: break
  prefix=source[prefix_start:line]
  found.append({"name":match.group(1),"body":source[opening+1:end-1],"masked":clean[opening+1:end-1],"test":"#[test]" in prefix,"gated":'#[cfg(feature = "parity-harness")]' in prefix})
 return found
def analyze_parity_source(source,label):
 funcs=rust_functions(source); dependent={item["name"] for item in funcs if "genesis_parity" in item["body"]}
 changed=True
 while changed:
  changed=False
  for item in funcs:
   if item["name"] in dependent: continue
   if any(re.search(rf'\b{re.escape(name)}\s*\(',item["masked"]) for name in dependent): dependent.add(item["name"]); changed=True
 missing=sorted(item["name"] for item in funcs if item["test"] and item["name"] in dependent and not item["gated"])
 extra=sorted(item["name"] for item in funcs if item["test"] and item["name"] not in dependent and item["gated"])
 if missing or extra: fail(f"{label} parity test custody drift: ungated={missing} overgated={extra}")
 return sum(item["test"] and item["gated"] for item in funcs)
def verify_parity_test_custody(root):
 files=0; gates=0
 for path in sorted((root/"crates/gc_cli/tests").glob("*.rs")):
  source=path.read_text()
  if "genesis_parity" not in source: continue
  files+=1; gates+=analyze_parity_source(source,str(path.relative_to(root)))
 if (files,gates)!=(33,105): fail(f"parity test custody inventory drift: files={files} gates={gates}")
 return {"parityTestFiles":files,"parityTests":gates}
def parity_custody_self_tests():
 good='fn parity() { genesis_parity(); }\n#[cfg(feature = "parity-harness")]\n#[test]\nfn uses_parity() { parity(); }\n#[test]\nfn production() {}\n'
 if analyze_parity_source(good,"self-test")!=1: fail("parity custody self-test count drift")
 controls=0
 for label,bad in (("transitive-ungated",good.replace('#[cfg(feature = "parity-harness")]\n',"")),("direct-ungated",good+'#[test]\nfn direct() { genesis_parity(); }\n'),("unrelated-overgated",good.replace('#[test]\nfn production','#[cfg(feature = "parity-harness")]\n#[test]\nfn production'))):
  try: analyze_parity_source(bad,label)
  except Error: controls+=1; continue
  fail(f"parity custody self-test accepted {label}")
 return controls
def verify_outer_manifests(root):
 packages={item["name"]:item for item in metadata(root).get("packages",[])}
 for pkg,production,parity in (("gc_cli","genesis","genesis_parity"),("gc_wasi_cli","genesis_wasi","genesis_wasi_parity")):
  data=packages.get(pkg,{})
  if data.get("features",{}).get("parity-harness") != ["dep:gc_cli_driver_parity"]: fail(f"{pkg} parity feature custody drift")
  if "parity-harness" in data.get("features",{}).get("default",[]): fail(f"{pkg} default features activate parity")
  dep=next((item for item in data.get("dependencies",[]) if item.get("name")=="gc_cli_driver_parity"),{})
  if dep.get("optional") is not True: fail(f"{pkg} parity driver is not optional")
  bins={item.get("name"):item for item in data.get("targets",[]) if "bin" in item.get("kind",[])}
  if set(bins.get(parity,{}).get("required-features",[])) != {"parity-harness"}: fail(f"{pkg} parity binary is not feature-gated")
  if bins.get(production,{}).get("required-features"): fail(f"{pkg} production binary unexpectedly requires a feature")
 gated_targets=sorted(item.get("name") for item in packages.get("gc_cli",{}).get("targets",[]) if "test" in item.get("kind",[]) and item.get("required-features"))
 if gated_targets: fail(f"whole integration targets must not be parity-gated: {gated_targets}")
 return verify_parity_test_custody(root)
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
 custody=verify_outer_manifests(root)
 for pkg in ("gc_cli","gc_wasi_cli"):
  package_tree=tree(root,pkg)
  if 'gc_patches feature "parity-oracle"' in package_tree or "gc_cli_driver_parity" in package_tree: fail(f"{pkg} production graph reaches the parity oracle")
 mains=(root/"crates/gc_cli/src/main.rs").read_text()+(root/"crates/gc_wasi_cli/src/main.rs").read_text()
 if "gc_cli_driver_parity" in mains or mains.count("gc_cli_driver::run")!=2: fail("production dispatch drift")
 schema=(root/"crates/gc_cli_driver/src/cli_schema.rs").read_text()
 if 'RuntimeProfile::Production => vec!["selfhost".to_string()]' not in schema: fail("production exposes non-selfhost frontend")
 return {"moduleCount":len(mods),"maxSourceLines":mx,"h2Eligible":True,**custody}
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
 edits=[("binding",lambda x:x["bindings"].pop()),("decision",lambda x:x["decisionInventory"].pop()),("source",lambda x:x["sourceModules"].pop()),("entrypoint",lambda x:x.__setitem__("productionEntrypoints",["genesis_parity"])),("oracle",lambda x:x["compatibilityOracle"].__setitem__("feature","default")),("eligibility",lambda x:x["releaseGraphDisposition"].__setitem__("h2Eligible",False)),("runtime",lambda x:x["runtimeEvidence"].__setitem__("lowStepControl",50000000)),("unknown",lambda x:x.__setitem__("unexpected",True))]
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
 validate(p,s); st=static(root,p); controls=(mutations(p,s)+parity_custody_self_tests()) if x.self_test else 0; rt=None
 if x.runtime:
  if not x.genesis_bin or not x.genesis_wasi_bin: fail("runtime binaries required")
  rt=runtime(root,p,[x.genesis_bin,x.genesis_wasi_bin])
 print(json.dumps({"kind":"genesis/selfhost-patch-authority-check-v0.1","ok":True,"profileIdentitySha256":ident(p),"static":st,"mutationControls":controls,"runtime":rt},sort_keys=True,separators=(",",":")))
if __name__=="__main__":
 try: main()
 except Error as e: print(f"selfhost-patch-authority: {e}",file=os.sys.stderr); raise SystemExit(1)
