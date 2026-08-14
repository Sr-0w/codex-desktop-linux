const fs = require("fs");
const path = require("path");

function requireName(source, moduleName) {
  const escaped = moduleName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return source.match(new RegExp(`([A-Za-z_$][\\w$]*)=require\\([\\\`'"]${escaped}[\\\`'"]\\)`))?.[1] ?? null;
}

function buildInstallAfterQuitSource(childProcessVar) {
  return `function codexLinuxInstallAfterQuit(){try{let e=${childProcessVar}.spawn(\`/bin/sh\`,[\`-c\`,\`for i in 1 2 3 4 5 6 7 8 9 10;do sleep 1;s="$("$1" status 2>/dev/null||true)";echo "$s"|grep -q "^status: WaitingForAppExit"&&continue;echo "$s"|grep -q "^status: Installing"&&continue;"$1" install-ready||exit $?;s="$("$1" status 2>/dev/null||true)";echo "$s"|grep -q "^status: WaitingForAppExit"&&continue;echo "$s"|grep -q "^status: Installing"&&continue;if echo "$s"|grep -q "^status: Installed";then (/usr/bin/codex-desktop >/dev/null 2>&1 &);fi;exit 0;done\`,\`codex-linux-update-install\`,codexLinuxUpdateManagerPath()],{detached:!0,stdio:\`ignore\`,windowsHide:!0});e.unref?.()}catch{}}`;
}

function replaceInstallAfterQuitSource(source, childProcessVar) {
  const pattern =
    /function codexLinuxInstallAfterQuit\(\)\{try\{let e=[A-Za-z_$][\w$]*\.spawn\(`\/bin\/sh`,\[`-c`,[^]*?e\.unref\?\.\(\)\}catch\{\}\}/;
  return source.replace(pattern, buildInstallAfterQuitSource(childProcessVar));
}

function replaceAfter(source, anchor, search, replacement) {
  const anchorIndex = source.indexOf(anchor);
  if (anchorIndex === -1) {
    return source;
  }
  const matchIndex = source.indexOf(search, anchorIndex);
  if (matchIndex === -1) {
    return source;
  }
  return source.slice(0, matchIndex) + replacement + source.slice(matchIndex + search.length);
}

function buildElectronResolverSource() {
  return "function codexLinuxGetElectronModule(){try{return require(`electron`)}catch{return null}}";
}

function buildUpToDateDetailSource() {
  return "function codexLinuxUpToDateDetail(e){try{let t=String(e?.installed_version??``).trim();return t&&t!==`unknown`?`Installed package ${t} is the latest available version.`:`You have the latest available version.`}catch{return`You have the latest available version.`}}";
}

function buildUpdateProgressHtml() {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Codex Desktop Update</title>
<style>
:root{color-scheme:light dark;font-family:Inter,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;background:#f6f7f9;color:#202124}
*{box-sizing:border-box}
body{margin:0;min-width:420px;min-height:330px;background:#f6f7f9;color:#202124}
main{display:flex;min-height:100vh;flex-direction:column;padding:28px 30px 24px}
header{display:flex;align-items:flex-start;justify-content:space-between;gap:20px}
h1{margin:0;font-size:20px;font-weight:650;line-height:1.25;letter-spacing:0}
.subtitle{margin:6px 0 0;color:#62666d;font-size:13px;line-height:1.45}
.percent{min-width:72px;text-align:right;font-size:28px;font-weight:650;font-variant-numeric:tabular-nums;letter-spacing:0}
.progress{height:8px;margin:24px 0 22px;overflow:hidden;border-radius:4px;background:#dfe2e7}
.progress>div{height:100%;width:0;border-radius:4px;background:#1769d2;transition:width .35s ease}
.status{display:grid;grid-template-columns:18px minmax(0,1fr);gap:12px;align-items:start}
.dot{width:10px;height:10px;margin-top:5px;border-radius:50%;background:#1769d2;box-shadow:0 0 0 4px rgba(23,105,210,.12)}
.dot.done{background:#18864b;box-shadow:0 0 0 4px rgba(24,134,75,.12)}
.dot.failed{background:#c7362f;box-shadow:0 0 0 4px rgba(199,54,47,.12)}
.step{font-size:15px;font-weight:600;line-height:1.4}
.state{margin-top:3px;color:#73777e;font-family:ui-monospace,SFMono-Regular,Consolas,monospace;font-size:11px;text-transform:uppercase}
.detail{min-height:44px;margin:18px 0 0;padding:14px 16px;border:1px solid #dfe2e7;border-radius:6px;background:#fff;color:#484c52;font-size:13px;line-height:1.45;overflow-wrap:anywhere}
.meta{display:flex;flex-wrap:wrap;gap:8px 18px;margin-top:14px;color:#73777e;font-size:12px}
.meta strong{color:#484c52;font-weight:600}
footer{display:flex;align-items:center;justify-content:space-between;gap:16px;margin-top:auto;padding-top:24px}
.elapsed{color:#73777e;font-size:12px;font-variant-numeric:tabular-nums}
button{min-width:84px;border:1px solid #b9bec6;border-radius:6px;padding:7px 16px;background:#fff;color:#202124;font:600 13px/1.3 inherit;cursor:pointer}
button:hover{background:#eef1f5}
button:focus-visible{outline:2px solid #1769d2;outline-offset:2px}
@media (prefers-color-scheme:dark){:root,body{background:#1d1f22;color:#f2f3f4}.subtitle,.state,.meta,.elapsed{color:#aeb3ba}.progress{background:#3a3d42}.detail{border-color:#3d4147;background:#26292d;color:#d7d9dc}.meta strong{color:#e4e6e8}button{border-color:#555a62;background:#2c2f34;color:#f2f3f4}button:hover{background:#373b41}}
</style>
</head>
<body>
<main>
<header><div><h1>Codex Desktop Update</h1><p class="subtitle">Checking and preparing the native Linux package</p></div><div class="percent" id="percent">0%</div></header>
<div class="progress" id="progress" role="progressbar" aria-label="Estimated update progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow="0"><div id="bar"></div></div>
<section class="status"><span class="dot" id="dot"></span><div><div class="step" id="step">Starting update check</div><div class="state" id="state">STARTING</div></div></section>
<div class="detail" id="detail">Connecting to the update manager...</div>
<div class="meta" id="meta"><span>Installed: <strong id="installed">Unknown</strong></span><span id="candidate-wrap" hidden>Candidate: <strong id="candidate"></strong></span></div>
<footer><span class="elapsed" id="elapsed">Elapsed 0:00</span><button id="close" type="button">Hide</button></footer>
</main>
<script>
const byId=id=>document.getElementById(id);
const percent=byId("percent"),progress=byId("progress"),bar=byId("bar"),dot=byId("dot"),step=byId("step"),state=byId("state"),detail=byId("detail"),installed=byId("installed"),candidate=byId("candidate"),candidateWrap=byId("candidate-wrap"),elapsed=byId("elapsed"),closeButton=byId("close");
closeButton.addEventListener("click",()=>window.close());
window.codexUpdateProgress=value=>{const p=Math.max(0,Math.min(100,Number(value.percent)||0));percent.textContent=p+"%";progress.setAttribute("aria-valuenow",String(p));bar.style.width=p+"%";step.textContent=value.step||"Updating Codex Desktop";state.textContent=String(value.status||"unknown").replaceAll("_"," ");detail.textContent=value.detail||"The update manager is working.";installed.textContent=value.installedVersion||"Unknown";candidate.textContent=value.candidateVersion||"";candidateWrap.hidden=!value.candidateVersion;elapsed.textContent="Elapsed "+Math.floor((value.elapsedSeconds||0)/60)+":"+String((value.elapsedSeconds||0)%60).padStart(2,"0");dot.className="dot"+(value.failed?" failed":value.terminal?" done":"");closeButton.textContent=value.terminal?"Close":"Hide"};
</script>
</body>
</html>`;
}

function buildUpdateProgressSource() {
  const html = JSON.stringify(buildUpdateProgressHtml());
  const runtimeSource = function codexLinuxUpdateProgressRuntime() {
    let codexLinuxUpdateProgressWindow = null;
    let codexLinuxUpdateProgressTimer = null;
    let codexLinuxUpdateProgressStartedAt = 0;
    let codexLinuxUpdateProgressCommandRunning = false;
    let codexLinuxUpdateProgressCommandFinished = false;
    let codexLinuxUpdateProgressError = null;
    const codexLinuxUpdateProgressHtml = "__CODEX_LINUX_UPDATE_PROGRESS_HTML__";

    function codexLinuxUpdateProgressActive(status) {
      return [
        "checking_upstream",
        "update_detected",
        "downloading_dmg",
        "preparing_workspace",
        "patching_app",
        "building_package",
        "installing",
      ].includes(status);
    }

    function codexLinuxUpdateProgressSnapshot(state) {
      let status =
        state?.status ?? (codexLinuxUpdateProgressCommandRunning ? "starting" : "idle");
      const stages = {
        starting: [2, "Starting update check", "Connecting to the update manager..."],
        checking_upstream: [
          8,
          "Checking upstream",
          "Comparing the installed build with the latest upstream DMG.",
        ],
        downloading_dmg: [
          20,
          "Downloading upstream DMG",
          "Downloading and verifying the official Codex Desktop image.",
        ],
        update_detected: [
          32,
          "Update detected",
          "A newer upstream build was found. Preparing the local rebuild.",
        ],
        preparing_workspace: [
          40,
          "Preparing build workspace",
          "Copying the packaged Linux builder and preparing a clean workspace.",
        ],
        patching_app: [
          58,
          "Building the Linux application",
          "Extracting the DMG, applying Linux compatibility patches, and rebuilding native modules.",
        ],
        building_package: [
          84,
          "Building the native package",
          "Packaging the rebuilt application for this Linux distribution.",
        ],
        ready_to_install: [
          100,
          "Update ready to install",
          "The native Linux package was built successfully and is ready to install.",
        ],
        waiting_for_app_exit: [
          100,
          "Update ready",
          "The package is ready and will be installed after Codex Desktop closes.",
        ],
        installing: [
          96,
          "Installing update",
          "Installing the rebuilt native package with system authentication.",
        ],
        installed: [
          100,
          "Update installed",
          "The latest Codex Desktop package is installed.",
        ],
        failed: [
          100,
          "Update failed",
          String(
            state?.error_message ??
              codexLinuxUpdateProgressError ??
              "The local Linux package could not be prepared.",
          ),
        ],
        idle: [100, "Codex Desktop is up to date", codexLinuxUpToDateDetail(state)],
      };
      let stage =
        stages[status] ??
        [
          codexLinuxUpdateProgressCommandRunning ? 5 : 100,
          "Updating Codex Desktop",
          "The update manager is working.",
        ];

      if (
        !codexLinuxUpdateProgressCommandFinished &&
        codexLinuxUpdateProgressCommandRunning &&
        !codexLinuxUpdateProgressActive(status)
      ) {
        status = "starting";
        stage = stages.starting;
      }

      const failed = status === "failed" || codexLinuxUpdateProgressError != null;
      const terminal =
        !codexLinuxUpdateProgressCommandRunning && !codexLinuxUpdateProgressActive(status);

      return {
        status: failed ? "failed" : status,
        step: failed ? "Update failed" : stage[1],
        detail: failed
          ? String(
              state?.error_message ??
                codexLinuxUpdateProgressError ??
                stage[2],
            )
          : stage[2],
        percent: stage[0],
        terminal,
        failed,
        candidateVersion: state?.candidate_version ?? null,
        installedVersion: state?.installed_version ?? null,
        elapsedSeconds: Math.max(
          0,
          Math.floor((Date.now() - codexLinuxUpdateProgressStartedAt) / 1000),
        ),
      };
    }

    function codexLinuxHasUpdateProgressWindow() {
      return (
        codexLinuxUpdateProgressWindow != null &&
        !codexLinuxUpdateProgressWindow.isDestroyed()
      );
    }

    function codexLinuxRenderUpdateProgress() {
      if (!codexLinuxHasUpdateProgressWindow()) {
        return;
      }
      const snapshot = codexLinuxUpdateProgressSnapshot(codexLinuxReadUpdateState());
      const script = `window.codexUpdateProgress?.(${JSON.stringify(snapshot)})`;
      codexLinuxUpdateProgressWindow.webContents
        .executeJavaScript(script, true)
        .catch(() => {});
    }

    function codexLinuxStopUpdateProgressMonitor() {
      if (codexLinuxUpdateProgressTimer != null) {
        clearInterval(codexLinuxUpdateProgressTimer);
        codexLinuxUpdateProgressTimer = null;
      }
    }

    function codexLinuxOpenUpdateProgress() {
      try {
        if (codexLinuxHasUpdateProgressWindow()) {
          codexLinuxUpdateProgressWindow.show();
          codexLinuxUpdateProgressWindow.focus();
          return codexLinuxUpdateProgressWindow;
        }

        const electron = codexLinuxGetElectronModule();
        if (!electron?.BrowserWindow) {
          return null;
        }

        const options = {
          width: 520,
          height: 390,
          minWidth: 460,
          minHeight: 340,
          show: false,
          resizable: true,
          title: "Codex Desktop Update",
          backgroundColor: electron.nativeTheme?.shouldUseDarkColors
            ? "#1d1f22"
            : "#f6f7f9",
          autoHideMenuBar: true,
          webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            sandbox: true,
          },
        };
        const parent = electron.BrowserWindow.getFocusedWindow?.();
        if (parent != null && !parent.isDestroyed()) {
          options.parent = parent;
        }

        codexLinuxUpdateProgressWindow = new electron.BrowserWindow(options);
        codexLinuxUpdateProgressWindow.setMenuBarVisibility?.(false);
        codexLinuxUpdateProgressWindow.on("closed", () => {
          codexLinuxUpdateProgressWindow = null;
          codexLinuxStopUpdateProgressMonitor();
        });
        codexLinuxUpdateProgressWindow
          .loadURL(
            `data:text/html;charset=utf-8,${encodeURIComponent(
              codexLinuxUpdateProgressHtml,
            )}`,
          )
          .then(() => {
            codexLinuxRenderUpdateProgress();
            codexLinuxUpdateProgressWindow?.show();
          })
          .catch(() => {});
        return codexLinuxUpdateProgressWindow;
      } catch {
        return null;
      }
    }

    function codexLinuxStartUpdateProgress() {
      codexLinuxUpdateProgressStartedAt = Date.now();
      codexLinuxUpdateProgressCommandRunning = true;
      codexLinuxUpdateProgressCommandFinished = false;
      codexLinuxUpdateProgressError = null;
      codexLinuxOpenUpdateProgress();
      codexLinuxStopUpdateProgressMonitor();
      codexLinuxUpdateProgressTimer = setInterval(() => {
        const state = codexLinuxReadUpdateState();
        codexLinuxRenderUpdateProgress();
        if (
          !codexLinuxUpdateProgressCommandRunning &&
          !codexLinuxUpdateProgressActive(state?.status)
        ) {
          codexLinuxStopUpdateProgressMonitor();
        }
      }, 500);
      codexLinuxUpdateProgressTimer.unref?.();
      codexLinuxRenderUpdateProgress();
    }

    function codexLinuxFinishUpdateProgress(error) {
      codexLinuxUpdateProgressCommandRunning = false;
      codexLinuxUpdateProgressCommandFinished = true;
      codexLinuxUpdateProgressError =
        error == null
          ? null
          : String(error?.stderr ?? error?.stdout ?? error?.message ?? error);
      codexLinuxRenderUpdateProgress();
      const state = codexLinuxReadUpdateState();
      if (!codexLinuxUpdateProgressActive(state?.status)) {
        codexLinuxStopUpdateProgressMonitor();
      }
    }

    async function codexLinuxRunUpdateManagerWithProgress(args) {
      codexLinuxStartUpdateProgress();
      try {
        const result = await codexLinuxRunUpdateManager(args);
        codexLinuxFinishUpdateProgress(null);
        return result;
      } catch (error) {
        codexLinuxFinishUpdateProgress(error);
        throw error;
      }
    }
  }.toString();
  const body = runtimeSource.slice(
    runtimeSource.indexOf("{") + 1,
    runtimeSource.lastIndexOf("}"),
  );
  return body.replace(
    '"__CODEX_LINUX_UPDATE_PROGRESS_HTML__"',
    html,
  );
}
function buildShowUpdateMessageSource(childProcessVar) {
  return `function codexLinuxUpdateErrorDetail(e){try{let t=codexLinuxReadUpdateState()?.error_message,n=e?.stderr??e?.stdout??e?.message??e,r=String(t??n??\`Unknown update error\`).trim();return r||\`Unknown update error\`}catch{return\`Unknown update error\`}}async function codexLinuxShowUpdateMessage(codexLinuxMessage,codexLinuxDetail){try{let e=String(codexLinuxMessage??\`Codex Desktop update\`),t=String(codexLinuxDetail??\`\`),n=String(process.env.XDG_CURRENT_DESKTOP??process.env.DESKTOP_SESSION??\`\`).toLowerCase();if(n.includes(\`kde\`)||n.includes(\`plasma\`)){let r=await new Promise(r=>{try{${childProcessVar}.execFile(\`kdialog\`,[\`--title\`,e,\`--msgbox\`,t],{windowsHide:!0},e=>r(e==null||e.code!==\`ENOENT\`&&e.code!==\`EACCES\`))}catch{r(!1)}});if(r)return}let r=codexLinuxGetElectronModule();if(!r)return;await r.dialog?.showMessageBox({type:\`info\`,buttons:[\`OK\`],defaultId:0,noLink:!0,message:e,detail:t})}catch{}}`;
}

function migrateShowUpdateMessageSource(source, childProcessVar) {
  const replacement = buildShowUpdateMessageSource(childProcessVar);
  if (
    source.includes("function codexLinuxUpdateErrorDetail(") &&
    source.includes("execFile(`kdialog`")
  ) {
    return source;
  }

  const resolverSource =
    "async function codexLinuxShowUpdateMessage(codexLinuxMessage,codexLinuxDetail){try{let e=codexLinuxGetElectronModule();if(!e)return;await e.dialog?.showMessageBox({type:`info`,buttons:[`OK`],defaultId:0,noLink:!0,message:codexLinuxMessage,detail:codexLinuxDetail})}catch{}}";
  const capturedElectronRegex =
    /async function codexLinuxShowUpdateMessage\(codexLinuxMessage,codexLinuxDetail\)\{try\{await [A-Za-z_$][\w$]*\.dialog\?\.showMessageBox\(\{type:`info`,buttons:\[`OK`\],defaultId:0,noLink:!0,message:codexLinuxMessage,detail:codexLinuxDetail\}\)\}catch\{\}\}/;

  if (source.includes(resolverSource)) {
    return source.replace(resolverSource, replacement);
  }
  return source.replace(capturedElectronRegex, replacement);
}

function buildQuitForUpdateSource(callInstallAfterQuit) {
  const prefix = callInstallAfterQuit ? "codexLinuxInstallAfterQuit();" : "";
  return `function codexLinuxQuitForUpdate(){try{${prefix}let t=codexLinuxGetElectronModule();if(!t)return;let e=setTimeout(()=>t.app?.exit?.(0),1500);e.unref?.(),t.app?.quit?.()}catch{}}`;
}

function buildBridgeSource({ childProcessVar, fsVar, pathVar }) {
  const showUpdateMessage = buildShowUpdateMessageSource(childProcessVar);
  const upToDateDetail = buildUpToDateDetailSource();
  const updateProgress = buildUpdateProgressSource();
  const installAfterQuit = buildInstallAfterQuitSource(childProcessVar);
  const quitForUpdate = buildQuitForUpdateSource(true);
  return `${buildElectronResolverSource()}function codexLinuxUpdateStatePath(){let e=process.env.XDG_STATE_HOME||process.env.HOME&&(0,${pathVar}.join)(process.env.HOME,\`.local\`,\`state\`);return e?(0,${pathVar}.join)(e,\`codex-update-manager\`,\`state.json\`):null}function codexLinuxReadUpdateState(){let e=codexLinuxUpdateStatePath();if(!e||!${fsVar}.existsSync(e))return null;try{let t=JSON.parse(${fsVar}.readFileSync(e,\`utf8\`));return t&&typeof t===\`object\`&&!Array.isArray(t)?t:null}catch{return null}}function codexLinuxUpdateLifecycleState(e){switch(e){case\`ready_to_install\`:case\`waiting_for_app_exit\`:return\`ready\`;case\`installing\`:return\`installing\`;case\`checking_upstream\`:case\`update_detected\`:case\`downloading_dmg\`:case\`preparing_workspace\`:case\`patching_app\`:case\`building_package\`:return\`checking\`;default:return\`idle\`}}function codexLinuxUpdateManagerPath(){let e=process.env.CODEX_UPDATE_MANAGER_PATH;return typeof e===\`string\`&&e.trim().length>0?e:\`codex-update-manager\`}${showUpdateMessage}${upToDateDetail}${installAfterQuit}${quitForUpdate}function codexLinuxRunUpdateManager(e){return new Promise((t,n)=>{${childProcessVar}.execFile(codexLinuxUpdateManagerPath(),e,{encoding:\`utf8\`,windowsHide:!0},(e,r,i)=>{if(e){e.stdout=r,e.stderr=i,n(e);return}t({stdout:r??\`\`,stderr:i??\`\`})})})}${updateProgress}async function codexLinuxProbeUpdateManager(){await codexLinuxRunUpdateManager([\`--help\`])}async function codexLinuxRefreshUpdateState(){return codexLinuxReadUpdateState()}`;
}

function packageUpdateManagerCompatibilityMethods() {
  return [
    ["latchInAppUpdatesEnabledForLaunch", "latchInAppUpdatesEnabledForLaunch:()=>{}"],
    ["setSparkleQueryParams", "setSparkleQueryParams:()=>{}"],
    ["getDownloadProgressPercent", "getDownloadProgressPercent:()=>null"],
    ["getDownloadedUpdateAppBrand", "getDownloadedUpdateAppBrand:()=>null"],
    ["getRelaunchNotice", "getRelaunchNotice:()=>null"],
  ];
}

function migratePackageUpdateManagerInterface(source) {
  const bootstrapIndex = source.indexOf("function codexLinuxCreatePackageUpdateManager(");
  if (bootstrapIndex === -1) {
    return source;
  }

  const managerIndex = source.indexOf("manager:{", bootstrapIndex);
  if (managerIndex === -1) {
    return source;
  }
  const managerEndIndex = source.indexOf("},quitForUpdate:", managerIndex);
  if (managerEndIndex === -1) {
    return source;
  }

  const managerSource = source.slice(managerIndex, managerEndIndex);
  const missingMethods = packageUpdateManagerCompatibilityMethods()
    .filter(([name]) => !managerSource.includes(`${name}:`))
    .map(([, methodSource]) => methodSource);
  if (missingMethods.length === 0) {
    return source;
  }

  const insertionIndex = managerIndex + "manager:{".length;
  return `${source.slice(0, insertionIndex)}${missingMethods.join(",")},${source.slice(insertionIndex)}`;
}

function migrateLinuxUpdaterBridgeSource(source, childProcessVar) {
  let patchedSource = migratePackageUpdateManagerInterface(
    migrateShowUpdateMessageSource(source, childProcessVar),
  ).replace(
    "async function codexLinuxRefreshUpdateState(){await codexLinuxRunUpdateManager([`status`,`--json`]);return codexLinuxReadUpdateState()}",
    "async function codexLinuxRefreshUpdateState(){return codexLinuxReadUpdateState()}",
  );
  if (
    patchedSource.includes("function codexLinuxRunUpdateManager(") &&
    !patchedSource.includes("function codexLinuxRunUpdateManagerWithProgress(")
  ) {
    patchedSource = patchedSource.replace(
      "async function codexLinuxProbeUpdateManager()",
      `${buildUpdateProgressSource()}async function codexLinuxProbeUpdateManager()`,
    );
  }
  patchedSource = patchedSource.split("await codexLinuxRunUpdateManager([`check-now`])").join(
    "await codexLinuxRunUpdateManagerWithProgress([`check-now`])",
  );
  if (
    patchedSource.includes("function codexLinuxReadUpdateState(") &&
    !patchedSource.includes("function codexLinuxUpToDateDetail(")
  ) {
    patchedSource = patchedSource.replace(
      "function codexLinuxUpdateErrorDetail(",
      `${buildUpToDateDetailSource()}function codexLinuxUpdateErrorDetail(`,
    );
  }
  patchedSource = patchedSource.replace(
    "await codexLinuxRunUpdateManagerWithProgress([`check-now`]),e()}catch(t){",
    "await codexLinuxRunUpdateManagerWithProgress([`check-now`]);let n=e();!this.isUpdateReady&&n&&(n.status===`idle`||n.status===`installed`)&&!codexLinuxHasUpdateProgressWindow()&&await codexLinuxShowUpdateMessage(`Codex Desktop is up to date`,codexLinuxUpToDateDetail(n))}catch(t){",
  );
  if (
    !patchedSource.includes(
      "this.setUpdateLifecycleState(this.isUpdateReady?`ready`:`idle`),codexLinuxHasUpdateProgressWindow()||await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(t))",
    )
  ) {
    patchedSource = patchedSource.replace(
      "this.setUpdateLifecycleState(this.isUpdateReady?`ready`:`idle`),await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(t))",
      "this.setUpdateLifecycleState(this.isUpdateReady?`ready`:`idle`),codexLinuxHasUpdateProgressWindow()||await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(t))",
    );
  }
  const probeSource =
    "async function codexLinuxProbeUpdateManager(){await codexLinuxRunUpdateManager([`--help`])}";
  const refreshSource =
    "async function codexLinuxRefreshUpdateState(){return codexLinuxReadUpdateState()}";
  if (
    patchedSource.includes("function codexLinuxRunUpdateManager(") &&
    patchedSource.includes(refreshSource) &&
    !patchedSource.includes(probeSource)
  ) {
    patchedSource = patchedSource.replace(
      refreshSource,
      `${probeSource}${refreshSource}`,
    );
  }

  const bootstrapNeedle = "function codexLinuxCreatePackageUpdateManager(";
  const isBootstrapSource = patchedSource.includes(bootstrapNeedle);
  if (
    patchedSource.includes("function codexLinuxRunUpdateManager(") &&
    isBootstrapSource &&
    (!patchedSource.includes(probeSource) || !patchedSource.includes(refreshSource))
  ) {
    const helperSource =
      `${patchedSource.includes(probeSource) ? "" : probeSource}` +
      `${patchedSource.includes(refreshSource) ? "" : refreshSource}`;
    patchedSource = patchedSource.replace(bootstrapNeedle, `${helperSource}${bootstrapNeedle}`);
  }

  patchedSource = patchedSource.replace(
    "await codexLinuxRefreshUpdateState(),e()",
    "await codexLinuxProbeUpdateManager(),e()",
  );

  const probeStateSource =
    "let s=!1,c=codexLinuxProbeUpdateManager().then(()=>{s=!0,i(),a();return!0}).catch(()=>{s=!1,t=!1,n=`idle`,a();return!1});let o=";
  const hasProbeState = () => patchedSource.includes("c=codexLinuxProbeUpdateManager().then(");
  if (isBootstrapSource && !hasProbeState() && patchedSource.includes(probeSource)) {
    patchedSource = replaceAfter(
      patchedSource,
      bootstrapNeedle,
      "i(),codexLinuxRefreshUpdateState().then(()=>{i(),a()}).catch(()=>{});let o=",
      probeStateSource,
    );
    patchedSource = replaceAfter(patchedSource, bootstrapNeedle, "i();let o=", probeStateSource);
  }

  if (!isBootstrapSource || !hasProbeState()) {
    return patchedSource;
  }

  patchedSource = replaceAfter(
    patchedSource,
    bootstrapNeedle,
    "getIsUpdateReady:()=>t,getUpdateLifecycleState:()=>n,",
    "getIsUpdateReady:()=>s&&t,getUpdateLifecycleState:()=>s?n:`idle`,",
  );
  patchedSource = replaceAfter(
    patchedSource,
    bootstrapNeedle,
    "checkForUpdates:async()=>{n=`checking`,a();try{",
    "checkForUpdates:async()=>{if(!await c)return;n=`checking`,a();try{",
  );
  patchedSource = replaceAfter(
    patchedSource,
    bootstrapNeedle,
    "await codexLinuxRunUpdateManagerWithProgress([`check-now`]),i(),a()",
    "await codexLinuxRunUpdateManagerWithProgress([`check-now`]);let u=i();a(),!t&&u&&(u.status===`idle`||u.status===`installed`)&&!codexLinuxHasUpdateProgressWindow()&&await codexLinuxShowUpdateMessage(`Codex Desktop is up to date`,codexLinuxUpToDateDetail(u))",
  );
  if (
    !patchedSource.includes(
      "n=t?`ready`:`idle`,a(),codexLinuxHasUpdateProgressWindow()||await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(e))",
    )
  ) {
    patchedSource = replaceAfter(
      patchedSource,
      "function codexLinuxCreatePackageUpdateManager(",
      "n=t?`ready`:`idle`,a(),await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(e))",
      "n=t?`ready`:`idle`,a(),codexLinuxHasUpdateProgressWindow()||await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(e))",
    );
  }
  patchedSource = replaceAfter(
    patchedSource,
    bootstrapNeedle,
    "installUpdatesIfAvailable:async()=>{i();if(!t){a();return}",
    "installUpdatesIfAvailable:async()=>{if(!await c){a();return}i();if(!t){a();return}",
  );
  patchedSource = replaceAfter(
    patchedSource,
    bootstrapNeedle,
    "installUpdatesIfAvailable:async()=>{i();if(!t)return;",
    "installUpdatesIfAvailable:async()=>{if(!await c){a();return}i();if(!t){a();return}",
  );
  patchedSource = replaceAfter(
    patchedSource,
    bootstrapNeedle,
    "refresh:async()=>{try{await codexLinuxRefreshUpdateState()}catch{}i(),a()}",
    "refresh:async()=>{if(await c){try{await codexLinuxRefreshUpdateState()}catch{}i()}else t=!1,n=`idle`;a()}",
  );
  return replaceAfter(
    patchedSource,
    bootstrapNeedle,
    "refresh:()=>{i(),a()}",
    "refresh:async()=>{if(await c){try{await codexLinuxRefreshUpdateState()}catch{}i()}else t=!1,n=`idle`;a()}",
  );
}

function buildBootstrapBridgeSource({ childProcessVar, fsVar, pathVar }) {
  const compatibilityMethods = packageUpdateManagerCompatibilityMethods()
    .map(([, methodSource]) => methodSource)
    .join(",");
  return `${buildBridgeSource({ childProcessVar, fsVar, pathVar })};function codexLinuxCreatePackageUpdateManager(e){let t=!1,n=\`idle\`,r=null,i=()=>{try{let e=codexLinuxReadUpdateState(),r=e?.status;t=r===\`ready_to_install\`||r===\`waiting_for_app_exit\`,n=codexLinuxUpdateLifecycleState(r);return e}catch{return null}},a=()=>{try{e.send({type:\`app-update-ready-changed\`,isUpdateReady:t}),e.send({type:\`app-update-lifecycle-state-changed\`,lifecycleState:n}),e.send({type:\`app-update-install-progress-changed\`,installProgressPercent:r})}catch{}},s=!1,c=codexLinuxProbeUpdateManager().then(()=>{s=!0,i(),a();return!0}).catch(()=>{s=!1,t=!1,n=\`idle\`,a();return!1});let o=()=>{e.allowQuit?.();codexLinuxQuitForUpdate()};return{manager:{${compatibilityMethods},setAutomaticBackgroundDownloadsEnabled:()=>{},getIsUpdateReady:()=>s&&t,getUpdateLifecycleState:()=>s?n:\`idle\`,getInstallProgressPercent:()=>r,checkForUpdates:async()=>{if(!await c)return;n=\`checking\`,a();try{await codexLinuxRunUpdateManager([\`check-now\`]),i(),a()}catch(e){n=t?\`ready\`:\`idle\`,a(),await codexLinuxShowUpdateMessage(\`Codex Desktop update failed\`,codexLinuxUpdateErrorDetail(e))}},installUpdatesIfAvailable:async()=>{if(!await c){a();return}i();if(!t){a();return}r=0,n=\`installing\`,a();try{let e=await codexLinuxRunUpdateManager([\`install-ready\`]),s=i();if(s?.status===\`waiting_for_app_exit\`){r=null,n=\`ready\`,a(),o();return}r=null,a(),e.stdout?.includes(\`Manual install required:\`)?await codexLinuxShowUpdateMessage(\`Codex Desktop update\`,e.stdout.trim()):e.stdout?.includes(\`already installed\`)?await codexLinuxShowUpdateMessage(\`Codex Desktop update\`,\`The ready update is already installed.\`):e.stdout?.includes(\`No Codex Desktop update is ready\`)&&await codexLinuxShowUpdateMessage(\`Codex Desktop update\`,\`There is no rebuilt update waiting to install.\`)}catch(e){r=null,n=t?\`ready\`:\`idle\`,a(),await codexLinuxShowUpdateMessage(\`Codex Desktop update failed\`,codexLinuxUpdateErrorDetail(e))}}},quitForUpdate:o,refresh:async()=>{if(await c){try{await codexLinuxRefreshUpdateState()}catch{}i()}else t=!1,n=\`idle\`;a()}}}`;
}

function applyCurrentBootstrapUpdaterBridgePatch(currentSource) {
  if (
    !currentSource.includes("setSparkleBridgeHandlers") ||
    !currentSource.includes("sparkleManager:") ||
    !currentSource.includes("onInstallUpdatesRequested")
  ) {
    return currentSource;
  }

  const childProcessVar =
    requireName(currentSource, "node:child_process") ?? requireName(currentSource, "child_process");
  const fsVar = requireName(currentSource, "node:fs") ?? requireName(currentSource, "fs");
  const pathVar = requireName(currentSource, "node:path") ?? requireName(currentSource, "path");
  if (childProcessVar == null || fsVar == null || pathVar == null) {
    console.warn("WARN: Could not find updater bridge module bindings - skipping Linux updater bridge patch");
    return currentSource;
  }

  let patchedSource = currentSource;
  if (!patchedSource.includes("function codexLinuxCreatePackageUpdateManager(")) {
    if (!patchedSource.includes("state:`disabled`")) {
      return currentSource;
    }
    const bootstrapMatch = patchedSource.match(/var [A-Za-z_$][\w$]*=\{enabled:!1,running:!1,state:`disabled`\};/);
    if (bootstrapMatch == null) {
      console.warn("WARN: Could not find current updater bridge insertion point - skipping Linux updater bridge patch");
      return currentSource;
    }
    patchedSource = patchedSource.replace(
      bootstrapMatch[0],
      `${buildBootstrapBridgeSource({ childProcessVar, fsVar, pathVar })};${bootstrapMatch[0]}`,
    );
  }

  patchedSource = migrateLinuxUpdaterBridgeSource(patchedSource, childProcessVar);

  const destructureRegex =
    /let\{startedAtMs:([A-Za-z_$][\w$]*),buildFlavor:([A-Za-z_$][\w$]*),desktopSentry:([A-Za-z_$][\w$]*),sparkleManager:([A-Za-z_$][\w$]*),[^{}]{0,1200}?setSparkleBridgeHandlers:([A-Za-z_$][\w$]*),[^{}]{0,1200}?setSecondInstanceArgsHandler:([A-Za-z_$][\w$]*)\}=([A-Za-z_$][\w$]*)\.([A-Za-z_$][\w$]*)\(\),/;
  const destructureMatch = patchedSource.match(destructureRegex);
  const sparkleVar = destructureMatch?.[4] ?? null;
  const setSparkleBridgeHandlersVar = destructureMatch?.[5] ?? null;
  if (sparkleVar == null) {
    console.warn("WARN: Could not identify current sparkleManager binding - skipping Linux updater bridge patch");
    return currentSource;
  }
  const bridgeHandlersStart = setSparkleBridgeHandlersVar == null
    ? -1
    : patchedSource.indexOf(`${setSparkleBridgeHandlersVar}({`, destructureMatch.index ?? 0);
  const bridgeHandlersSearchSource = bridgeHandlersStart === -1
    ? ""
    : patchedSource.slice(bridgeHandlersStart);
  const messageDispatcherVar = bridgeHandlersSearchSource.match(
    /([A-Za-z_$][\w$]*)\.sendMessageToAllRegisteredWindows\(\{type:`app-update-ready-changed`/,
  )?.[1] ?? null;
  const appUpdateStateBroadcasterVar = bridgeHandlersSearchSource.match(
    /([A-Za-z_$][\w$]*)\.broadcastAppUpdateState\(\)/,
  )?.[1] ?? null;
  const sendCallback = messageDispatcherVar == null
    ? appUpdateStateBroadcasterVar == null
      ? null
      : `send:()=>${appUpdateStateBroadcasterVar}.broadcastAppUpdateState()`
    : `send:e=>${messageDispatcherVar}.sendMessageToAllRegisteredWindows(e)`;
  if (sendCallback == null) {
    console.warn("WARN: Could not identify current updater window message dispatcher - skipping Linux updater bridge patch");
    return currentSource;
  }

  if (!patchedSource.includes("codexLinuxPackageUpdateBridge=process.platform===`linux`")) {
    const legacyBridgeRegex =
      /let ([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\(\),([A-Za-z_$][\w$]*)=\(\)=>\{\1\.allowQuitTemporarilyForUpdateInstall\(\),([A-Za-z_$][\w$]*)\.app\.quit\(\)\};/;
    if (legacyBridgeRegex.test(patchedSource)) {
      patchedSource = patchedSource.replace(
        legacyBridgeRegex,
        (_match, quitControllerVar, quitFactoryVar, quitFnVar, electronBindingVar) =>
          `let ${quitControllerVar}=${quitFactoryVar}(),${quitFnVar}=()=>{${quitControllerVar}.allowQuitTemporarilyForUpdateInstall(),${electronBindingVar}.app.quit()},codexLinuxPackageUpdateBridge=process.platform===\`linux\`?codexLinuxCreatePackageUpdateManager({allowQuit:()=>${quitControllerVar}.allowQuitTemporarilyForUpdateInstall(),${sendCallback}}):null;codexLinuxPackageUpdateBridge!=null&&(${sparkleVar}=codexLinuxPackageUpdateBridge.manager,${quitFnVar}=codexLinuxPackageUpdateBridge.quitForUpdate,setInterval(()=>codexLinuxPackageUpdateBridge.refresh(),3e4).unref?.());`,
      );
    } else {
      const currentBridgeRegex =
        /let ([A-Za-z_$][\w$]*)=(?:new )?[A-Za-z_$][\w$]*(?:\(\))?,(?:[A-Za-z_$][\w$]*=null,)+([A-Za-z_$][\w$]*)=[A-Za-z_$][\w$]*=>\{[^]*?\}(?=,|;)/;
      const currentBridgeMatch = patchedSource.match(currentBridgeRegex);
      if (currentBridgeMatch == null) {
        console.warn("WARN: Could not find current updater callback bridge - skipping Linux updater bridge patch");
        return currentSource;
      }
      const [bridgeDeclaration, quitControllerVar, quitFnVar] = currentBridgeMatch;
      const bridgeSetup =
        `${bridgeDeclaration},codexLinuxPackageUpdateBridge=process.platform===\`linux\`?codexLinuxCreatePackageUpdateManager({allowQuit:()=>${quitControllerVar}.allowQuitTemporarilyForUpdateInstall(),${sendCallback}}):null,codexLinuxPackageUpdateBridgeSetup=codexLinuxPackageUpdateBridge!=null&&(${sparkleVar}=codexLinuxPackageUpdateBridge.manager,${quitFnVar}=codexLinuxPackageUpdateBridge.quitForUpdate,setInterval(()=>codexLinuxPackageUpdateBridge.refresh(),3e4).unref?.())`;
      patchedSource = patchedSource.replace(currentBridgeRegex, bridgeSetup);
    }
  }

  return patchedSource;
}

function applyLinuxAppUpdaterBridgePatch(currentSource) {
  const currentBootstrapPatched = applyCurrentBootstrapUpdaterBridgePatch(currentSource);
  if (currentBootstrapPatched !== currentSource) {
    return currentBootstrapPatched;
  }

  if (!currentSource.includes("var tD=class{") || !currentSource.includes("initializeMacSparkle")) {
    return currentSource;
  }

  const childProcessVar =
    requireName(currentSource, "node:child_process") ?? requireName(currentSource, "child_process");
  const fsVar = requireName(currentSource, "node:fs") ?? requireName(currentSource, "fs");
  const pathVar = requireName(currentSource, "node:path") ?? requireName(currentSource, "path");
  if (childProcessVar == null || fsVar == null || pathVar == null) {
    console.warn("WARN: Could not find updater bridge module bindings - skipping Linux updater bridge patch");
    return currentSource;
  }

  let patchedSource = currentSource;
  if (!patchedSource.includes("function codexLinuxUpdateLifecycleState(")) {
    const classNeedle = "var tD=class{";
    patchedSource = patchedSource.replace(
      classNeedle,
      `${buildBridgeSource({ childProcessVar, fsVar, pathVar })};${classNeedle}`,
    );
  }
  if (!patchedSource.includes("function codexLinuxGetElectronModule(")) {
    const updateStateNeedle = "function codexLinuxUpdateStatePath(";
    if (patchedSource.includes(updateStateNeedle)) {
      patchedSource = patchedSource.replace(updateStateNeedle, `${buildElectronResolverSource()}${updateStateNeedle}`);
    }
  }
  patchedSource = migrateShowUpdateMessageSource(patchedSource, childProcessVar);
  if (!patchedSource.includes("function codexLinuxQuitForUpdate(")) {
    const quitSource = `${buildInstallAfterQuitSource(childProcessVar)}${buildQuitForUpdateSource(true)}`;
    const runManagerNeedle = "function codexLinuxRunUpdateManager(";
    if (patchedSource.includes(runManagerNeedle)) {
      patchedSource = patchedSource.replace(runManagerNeedle, `${quitSource}${runManagerNeedle}`);
    }
  } else {
    if (!patchedSource.includes("function codexLinuxInstallAfterQuit(")) {
      patchedSource = patchedSource.replace(
        "function codexLinuxQuitForUpdate(",
        `${buildInstallAfterQuitSource(childProcessVar)}function codexLinuxQuitForUpdate(`,
      );
    }
    patchedSource = patchedSource
      .replace(
        /function codexLinuxQuitForUpdate\(\)\{try\{let e=setTimeout\(\(\)=>[A-Za-z_$][\w$]*\.app\?\.exit\?\.\(0\),1500\);e\.unref\?\.\(\),[A-Za-z_$][\w$]*\.app\?\.quit\?\.\(\)\}catch\{\}\}/,
        buildQuitForUpdateSource(true),
      )
      .replace(
        /function codexLinuxQuitForUpdate\(\)\{try\{codexLinuxInstallAfterQuit\(\);let e=setTimeout\(\(\)=>[A-Za-z_$][\w$]*\.app\?\.exit\?\.\(0\),1500\);e\.unref\?\.\(\),[A-Za-z_$][\w$]*\.app\?\.quit\?\.\(\)\}catch\{\}\}/,
        buildQuitForUpdateSource(true),
      );
  }
  if (patchedSource.includes("function codexLinuxInstallAfterQuit(")) {
    patchedSource = replaceInstallAfterQuitSource(patchedSource, childProcessVar);
  }
  patchedSource = patchedSource.replace(
    "this.setInstallProgressPercent(null),this.options.onInstallUpdatesRequested?.();return",
    "this.setInstallProgressPercent(null),codexLinuxQuitForUpdate();return",
  );

  const initializeNeedle =
    "if(process.platform===`win32`?await this.initializeWindowsUpdater():await this.initializeMacSparkle(),t.ipcMain.handle(";
  const initializePatch =
    "if(process.platform===`linux`?await this.initializeLinuxPackageUpdater():process.platform===`win32`?await this.initializeWindowsUpdater():await this.initializeMacSparkle(),t.ipcMain.handle(";
  if (patchedSource.includes(initializePatch)) {
    // Already patched.
  } else if (patchedSource.includes(initializeNeedle)) {
    patchedSource = patchedSource.replace(initializeNeedle, initializePatch);
  } else {
    console.warn("WARN: Could not find updater initialize platform branch - skipping Linux updater bridge patch");
    return currentSource;
  }

  const disabledGateNeedle = "if(!this.options.enableUpdater){this.lastUnavailableReason=process.platform!==`darwin`&&process.platform!==`win32`?";
  const disabledGatePatch = "if(!this.options.enableUpdater&&process.platform!==`linux`){this.lastUnavailableReason=process.platform!==`darwin`&&process.platform!==`win32`?";
  if (patchedSource.includes(disabledGatePatch)) {
    // Already patched.
  } else if (patchedSource.includes(disabledGateNeedle)) {
    patchedSource = patchedSource.replace(disabledGateNeedle, disabledGatePatch);
  } else {
    console.warn("WARN: Could not find updater enable gate - skipping Linux updater enable patch");
    return currentSource;
  }

  if (!patchedSource.includes("async initializeLinuxPackageUpdater(){")) {
    const methodNeedle = "async initializeWindowsUpdater(){";
    const methodPatch =
      "async initializeLinuxPackageUpdater(){if(process.platform!==`linux`){this.lastUnavailableReason=`unsupported platform`;return}let e=()=>{let e=codexLinuxReadUpdateState(),t=e?.status;this.setUpdateReady(t===`ready_to_install`||t===`waiting_for_app_exit`),this.setUpdateLifecycleState(codexLinuxUpdateLifecycleState(t)),this.lastUnavailableReason=null;return e};try{await codexLinuxProbeUpdateManager(),e()}catch(e){this.lastUnavailableReason=e?.code===`ENOENT`?`codex-update-manager not found`:`codex-update-manager unavailable`,ZE().warning(`Linux updater unavailable`,{safe:{reason:this.lastUnavailableReason},sensitive:{error:e}});return}this.updater={setAutomaticBackgroundDownloadsEnabled:()=>{},checkForUpdates:async()=>{this.setUpdateLifecycleState(`checking`);try{await codexLinuxRunUpdateManager([`check-now`]),e()}catch(t){this.setUpdateLifecycleState(this.isUpdateReady?`ready`:`idle`),await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(t))}},installUpdatesIfAvailable:async()=>{e();if(!this.isUpdateReady)return;this.setInstallProgressPercent(0),this.setUpdateLifecycleState(`installing`);try{let n=await codexLinuxRunUpdateManager([`install-ready`]),t=e();if(t?.status===`waiting_for_app_exit`){this.setInstallProgressPercent(null),codexLinuxQuitForUpdate();return}this.setInstallProgressPercent(null),n.stdout?.includes(`already installed`)?await codexLinuxShowUpdateMessage(`Codex Desktop update`,`The ready update is already installed.`):n.stdout?.includes(`No Codex Desktop update is ready`)&&await codexLinuxShowUpdateMessage(`Codex Desktop update`,`There is no rebuilt update waiting to install.`)}catch(e){this.setInstallProgressPercent(null),this.setUpdateLifecycleState(this.isUpdateReady?`ready`:`idle`),await codexLinuxShowUpdateMessage(`Codex Desktop update failed`,codexLinuxUpdateErrorDetail(e))}}};let t=setInterval(()=>{codexLinuxRefreshUpdateState().then(()=>e()).catch(e=>{ZE().warning(`Linux updater state refresh failed`,{safe:{},sensitive:{error:e}})})},3e4);t.unref?.()}";
    if (!patchedSource.includes(methodNeedle)) {
      console.warn("WARN: Could not find updater method insertion point - skipping Linux updater bridge patch");
      return currentSource;
    }
    patchedSource = patchedSource.replace(methodNeedle, `${methodPatch}${methodNeedle}`);
  }

  return migrateLinuxUpdaterBridgeSource(patchedSource, childProcessVar);
}

function applyLinuxAppUpdaterMenuPatch(currentSource) {
  let patchedSource = currentSource;
  const hasLinuxUpdaterGate =
    /[A-Za-z_$][\w$]*=[A-Za-z_$][\w$]*\.[A-Za-z_$][\w$]*\.shouldIncludeSparkle\([A-Za-z_$][\w$]*,process\.platform,process\.env\)\|\|process\.platform===`linux`/.test(
      patchedSource,
    );
  const menuRegex =
    /([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*)\.([A-Za-z_$][\w$]*)\.shouldIncludeSparkle\(([A-Za-z_$][\w$]*),process\.platform,process\.env\)/;
  if (!hasLinuxUpdaterGate && menuRegex.test(patchedSource)) {
    patchedSource = patchedSource.replace(
      menuRegex,
      "$1=$2.$3.shouldIncludeSparkle($4,process.platform,process.env)||process.platform===`linux`",
    );
  } else if (
    !hasLinuxUpdaterGate &&
    patchedSource.includes("enableSparkle") &&
    patchedSource.includes("shouldIncludeSparkle")
  ) {
    console.warn("WARN: Could not find update menu feature gate - skipping Linux update menu patch");
  }

  const updateLabelIndex = patchedSource.indexOf("defaultMessage:`Check for Updates");
  if (updateLabelIndex !== -1) {
    const helpMenuTail = patchedSource.slice(updateLabelIndex);
    const linuxExcludedItemRegex =
      /\.\.\.([A-Za-z_$][\w$]*)&&process\.platform!==`linux`\?\[([A-Za-z_$][\w$]*)\]:\[\]/;
    const linuxExcludedItemMatch = helpMenuTail.match(linuxExcludedItemRegex);
    if (linuxExcludedItemMatch != null) {
      const [match, featureGate, updateItem] = linuxExcludedItemMatch;
      const matchIndex = updateLabelIndex + linuxExcludedItemMatch.index;
      const replacement =
        `...(process.platform===\`linux\`||${featureGate})?[${updateItem}]:[]`;
      patchedSource =
        patchedSource.slice(0, matchIndex) +
        replacement +
        patchedSource.slice(matchIndex + match.length);
    }
  }

  return patchedSource;
}

function patchLinuxAppUpdaterBridge(extractedDir) {
  const buildDir = path.join(extractedDir, ".vite", "build");
  if (!fs.existsSync(buildDir)) {
    console.warn(`WARN: Could not find build directory in ${buildDir} - skipping Linux updater bridge patch`);
    return { matched: 0, changed: 0 };
  }

  let matched = 0;
  let changed = 0;
  for (const fileName of fs.readdirSync(buildDir).filter((name) => name.endsWith(".js")).sort()) {
    const filePath = path.join(buildDir, fileName);
    const source = fs.readFileSync(filePath, "utf8");
    const shouldPatchMenu = source.includes("shouldIncludeSparkle");
    const shouldPatchBridge =
      source.includes("exports.runMainAppStartup") ||
      source.includes("var tD=class{");
    if (!shouldPatchMenu && !shouldPatchBridge) {
      continue;
    }
    matched += 1;
    let patched = source;
    if (shouldPatchMenu) {
      patched = applyLinuxAppUpdaterMenuPatch(patched);
    }
    if (shouldPatchBridge) {
      patched = applyLinuxAppUpdaterBridgePatch(patched);
    }
    if (patched !== source) {
      fs.writeFileSync(filePath, patched, "utf8");
      changed += 1;
    }
  }

  return { matched, changed };
}

module.exports = {
  applyLinuxAppUpdaterBridgePatch,
  applyLinuxAppUpdaterMenuPatch,
  patchLinuxAppUpdaterBridge,
};
