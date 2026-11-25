// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
// node axe-check.js <base-url>   (needs: npm i playwright-core axe-core; env CHROME=<browser path>)
const { chromium } = require("playwright-core");const fs=require("fs");
const SNAP=process.env.SNAP||"./api-snap";const sysj=JSON.parse(fs.readFileSync(SNAP+"/system.json"));sysj.t="sys";const statj=JSON.parse(fs.readFileSync(SNAP+"/status.json"));
const fakeInit=()=>`window.WebSocket=class{constructor(){setTimeout(()=>this.onmessage&&this.onmessage({data:JSON.stringify(${JSON.stringify(sysj)})}),50);setInterval(()=>this.onmessage&&this.onmessage({data:JSON.stringify(${JSON.stringify(statj)})}),100)}send(){}close(){}};localStorage.setItem("pilab.token","mock");`;
const axe=fs.readFileSync(require.resolve("axe-core/axe.min.js"),"utf8");
(async()=>{const b=await chromium.launch({executablePath:process.env.CHROME||undefined,args:["--no-sandbox"]});
const base=process.argv[2];
for (const [w,h] of [[1440,900],[390,844]]) {
const ctx=await b.newContext({viewport:{width:w,height:h}});await ctx.addInitScript(fakeInit());
const pg=await ctx.newPage();
for (const v of ["overview","sounds","midimap","scenes","signals","devices","settings"]) {
 await pg.goto(base+"/#"+v);await pg.reload();await pg.waitForTimeout(1500);
 await pg.evaluate(axe);
 const r=await pg.evaluate(async()=>{const r=await axe.run(document,{runOnly:["wcag2a","wcag2aa","best-practice"]});return r.violations.map(x=>({id:x.id,impact:x.impact,n:x.nodes.length,ex:x.nodes[0].target.join(" ").slice(0,80)}))});
 console.log(w,v,JSON.stringify(r));
}
await ctx.close();}
await b.close();})();
