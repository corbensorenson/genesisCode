#!/usr/bin/env python3
import argparse,json,sys,time
p=argparse.ArgumentParser(); p.add_argument("--respond",action="store_true"); p.add_argument("--hang",action="store_true"); a=p.parse_args()
if a.hang:
    time.sleep(3600)
request=json.load(sys.stdin)
response={"candidateFiles":[{"contentBase64":"NDIK","path":"main.gc"}],"finishReason":"stop","providerFacts":{"runtime-build":"fixture-v0.1"},"requestIdentitySha256":request["contentIdentitySha256"],"status":"succeeded","usage":{"inputTokens":0,"outputTokens":1}}
json.dump(response,sys.stdout,sort_keys=True,separators=(",",":")); sys.stdout.write("\n")
