const state = document.querySelector("#host-state");
const trustBadge = document.querySelector("#trust-badge");
const verdict = document.querySelector("#verdict");
const identityDetails = document.querySelector("#identity-details");
const cryptoDetails = document.querySelector("#crypto-details");
const capabilityList = document.querySelector("#capability-list");
const actions = document.querySelector("#actions");
const alwaysButton = document.querySelector("#always-button");
const actionStatus = document.querySelector("#action-status");
const boundaryTitle = document.querySelector("#boundary-title");
const adminOutput = document.querySelector("#admin-output");
const forgetDecisionButton = document.querySelector("#forget-decision-button");
const revokeButton = document.querySelector("#revoke-button");
const supportButton = document.querySelector("#support-button");
const lifecycleStatus = document.querySelector("#lifecycle-status");
const lifecycleActionStatus = document.querySelector("#lifecycle-action-status");
const updateStatus = document.querySelector("#update-status");
const updateCheckButton = document.querySelector("#update-check-button");
const updateDownloadControls = document.querySelector("#update-download-controls");
const updateDownloadConsent = document.querySelector("#update-download-consent");
const updateDownloadButton = document.querySelector("#update-download-button");
const updateStageControls = document.querySelector("#update-stage-controls");
const updateStageConsent = document.querySelector("#update-stage-consent");
const updateStageButton = document.querySelector("#update-stage-button");
const updateValidationStatus = document.querySelector("#update-validation-status");
const openButton = document.querySelector("#open-button");
const reopenButton = document.querySelector("#reopen-button");
const readOnlyButton = document.querySelector("#read-only-button");
const restoreButton = document.querySelector("#restore-button");
const promptTitle = document.querySelector("#prompt-title");
let currentBackupId = null;
let lastFocusKey = null;
let reviewedUpdateVersion = null;

const trustLabels = {
  unverified: "Unverified",
  structurally_verified_unsigned: "Verified structure · unsigned",
  signature_valid_unknown_publisher: "Valid signature · unknown publisher",
  signed_trusted_publisher: "Signed · trusted publisher",
  locally_trusted_exact_release: "Locally trusted exact release",
  modified_after_signature: "Modified after signature",
  invalid_signature: "Invalid signature",
  denied_by_user: "Denied by you",
  revoked: "Revoked",
};

function setRows(target, rows) {
  target.replaceChildren(...rows.map(([term, value]) => {
    const row = document.createElement("div");
    const dt = document.createElement("dt");
    const dd = document.createElement("dd");
    dt.textContent = term;
    dd.textContent = String(value);
    row.append(dt, dd);
    return row;
  }));
}

function renderUpdateStatus(report) {
  const attention = report.incomplete_artifacts.length + report.invalid_artifacts.length;
  updateCheckButton.disabled = !report.transport_configured || Boolean(report.error) || Boolean(attention);
  reviewedUpdateVersion = report.release_policy_verified ? report.candidate_version : null;
  updateDownloadControls.hidden = !reviewedUpdateVersion || report.downloaded;
  updateDownloadConsent.checked = false;
  updateDownloadButton.disabled = true;
  updateStageControls.hidden = !report.downloaded || Boolean(report.state);
  updateStageConsent.checked = false;
  updateStageButton.disabled = true;
  if (report.downloaded) {
    updateValidationStatus.textContent = `Downloaded ${report.downloaded_artifact_bytes.toLocaleString()} package bytes and ${report.downloaded_sigstore_bundle_bytes.toLocaleString()} Sigstore-evidence bytes. Transport, signed digests, the pinned platform signer (${report.platform_signer_subject}), and Sigstore identity ${report.sigstore_certificate_identity} all match. The update is verified but remains unstaged and uninstalled.`;
  } else if (report.release_policy_verified) {
    updateValidationStatus.textContent = `Release sequence ${report.candidate_sequence} passed the compiled Ed25519 release policy. Download still requires your explicit consent.`;
  } else {
    updateValidationStatus.textContent = "";
  }
  if (report.error || attention) {
    const detail = report.error || `${report.incomplete_artifacts.length} interrupted and ${report.invalid_artifacts.length} invalid stage artifacts`;
    updateStatus.textContent = `Host update state requires attention: ${detail}. No update is treated as healthy.`;
    updateStatus.className = "error";
    return;
  }
  updateStatus.className = "";
  if (!report.state) {
    updateStatus.textContent = report.transport_configured
      ? `Host ${report.current_version} · no staged update. A pinned host-only updater is compiled for ${report.transport_endpoint_origin}.`
      : `Host ${report.current_version} · no staged update. This development build has no complete compiled updater trust configuration.`;
    return;
  }
  const candidate = report.candidate_version || "unknown version";
  const messages = {
    prepared: `Host ${report.current_version} · update ${candidate} is verified and staged. Installation has not started and still requires explicit consent.`,
    installer_started: `Update ${candidate} started but did not reach the startup-health boundary. Rollback is required.`,
    awaiting_health: `Update ${candidate} is awaiting trusted-shell startup health.`,
    healthy: `Host ${report.current_version} · update ${candidate} passed the trusted-shell startup-health check.`,
    rollback_required: report.rollback_available
      ? `Update ${candidate} requires rollback. The exact preserved prior installer is available to the platform adapter.`
      : `Update ${candidate} requires rollback, but no prior installer is inventoried. Automatic rollback is unavailable.`,
  };
  updateStatus.textContent = messages[report.state] || `Host update state: ${report.state}.`;
}

function shortDigest(value) {
  return value ? `${value.slice(0, 12)}…${value.slice(-10)}` : "Not available";
}

function renderCapabilities(capsule) {
  const permissions = capsule.identity.permissions || {};
  const decisions = capsule.decision.capabilities || {};
  const rows = Object.entries(decisions).map(([name, evaluation]) => {
    const declaration = permissions[name] || {};
    const label = document.createElement("label");
    label.className = "capability";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.value = name;
    checkbox.checked = evaluation.supported && evaluation.decision !== "deny";
    checkbox.disabled = !evaluation.supported;
    const copy = document.createElement("span");
    const title = document.createElement("strong");
    const reason = document.createElement("p");
    title.textContent = name;
    reason.textContent = declaration.reason || evaluation.reason;
    copy.append(title, reason);
    const kind = document.createElement("span");
    kind.className = evaluation.required ? "required" : "optional";
    kind.textContent = evaluation.required ? "Required" : "Optional";
    label.append(checkbox, copy, kind);
    return label;
  });
  capabilityList.replaceChildren(...rows);
}

function setVerdict(title, copy, kind, mark) {
  verdict.className = `verdict ${kind || ""}`.trim();
  const icon = document.createElement("span");
  icon.className = "verdict-mark";
  icon.setAttribute("aria-hidden", "true");
  icon.textContent = mark;
  const text = document.createElement("div");
  const strong = document.createElement("strong");
  const paragraph = document.createElement("p");
  strong.textContent = title;
  paragraph.textContent = copy;
  text.append(strong, paragraph);
  verdict.replaceChildren(icon, text);
}

function focusKeyFor(report) {
  const identity = report.capsule?.source_sha256 || report.error || "no-capsule";
  return `${report.stage}:${identity}`;
}

function renderReport(report) {
  const focusKey = focusKeyFor(report);
  actionStatus.textContent = "";
  actionStatus.className = "action-status";
  if (report.error) {
    state.textContent = "Rejected before execution";
    trustBadge.textContent = "Fail closed";
    trustBadge.className = "badge fail";
    setVerdict("Capsule rejected", report.error, "fail", "×");
    setRows(identityDetails, [["Stage", report.stage], ["Executable assets", "Not released"]]);
    capabilityList.replaceChildren();
    actions.querySelectorAll("button").forEach((button) => { button.disabled = true; });
    reopenButton.disabled = true;
    readOnlyButton.disabled = true;
    forgetDecisionButton.disabled = true;
    if (lastFocusKey !== focusKey) {
      verdict.focus();
      lastFocusKey = focusKey;
    }
    return;
  }
  if (!report.capsule) {
    state.textContent = "No capsule selected";
    trustBadge.textContent = "Idle";
    setVerdict("Choose a capsule to begin", "Open with a .capsule.sqlite path. Nothing is executing.", "", "·");
    setRows(identityDetails, [["Stage", report.stage], ["Executable assets", "Not released"]]);
    capabilityList.replaceChildren();
    actions.querySelectorAll("button").forEach((button) => { button.disabled = true; });
    reopenButton.disabled = true;
    readOnlyButton.disabled = true;
    forgetDecisionButton.disabled = true;
    if (lastFocusKey !== focusKey) {
      openButton.focus();
    }
    lastFocusKey = focusKey;
    return;
  }

  const capsule = report.capsule;
  const decision = capsule.decision;
  const identity = capsule.identity;
  const recovery = report.recovery;
  const recoverySummary = recovery
    ? `SQLite recovery attempted · ${recovery.rollback_journal_hot_candidate_before ? "hot-journal candidate" : "rollback sidecar"} · ${recovery.rollback_journal_present_after ? "sidecar retained" : "sidecar cleared by SQLite"}`
    : "Not required";
  const label = trustLabels[decision.trust_state] || decision.trust_state;
  const blocked = ["unverified", "modified_after_signature", "invalid_signature", "denied_by_user", "revoked"].includes(decision.trust_state);
  const authorized = decision.executable_allowed;
  reopenButton.disabled = false;
  readOnlyButton.disabled = !authorized;
  state.textContent = authorized && capsule.assets_released
    ? recovery
      ? "Recovered and verified application running · named bridge only"
      : "Verified application running · named bridge only"
    : authorized
      ? "Policy authorized · payload still locked"
      : "Trust decision required · code locked";
  trustBadge.textContent = label;
  trustBadge.className = `badge ${blocked ? "fail" : authorized ? "ok" : "warn"}`;
  if (authorized && capsule.assets_released) {
    setVerdict(
      recovery ? "Recovered state verified; assets released" : "Verified assets released",
      recovery
        ? "SQLite performed rollback-journal recovery. The host then repeated integrity, signature, policy, and capsule checks before releasing the named bridge."
        : "The raw renderer can reach only manifest, effective permissions, and named read/write operations through its per-launch session.",
      "ok",
      "✓",
    );
  } else if (authorized) {
    setVerdict("Policy allows this capability set", "The application payload remains locked because the verified runtime bridge did not open.", "", "?");
  } else if (blocked) {
    setVerdict("Execution is blocked", "The trust state denies application capabilities. No executable assets were released.", "fail", "!");
  } else {
    setVerdict("Your decision is required", "Review the untrusted title, verified identity, publisher status, and each capability below.", "", "?");
  }

  setRows(identityDetails, [
    ["Untrusted title", identity.title],
    ["File", identity.canonical_path],
    ["Capsule", identity.capsule_id],
    ["Application", `${identity.app_id} ${identity.app_version}`],
    ["Publisher", capsule.publisher ? capsule.publisher.name : "No signed publisher identity"],
    ["Crash recovery", recoverySummary],
    ["Executable assets", capsule.assets_released ? "Released" : "Not released"],
  ]);
  const signature = capsule.signatures[0];
  setRows(cryptoDetails, [
    ["File SHA-256", capsule.source_sha256],
    ["Application digest", capsule.application_digest || "Unsigned"],
    ["Publisher ID", capsule.publisher?.id || "None"],
    ["Key fingerprint", signature?.key_id || "None"],
    ["Signature", decision.signature_valid ? "Cryptographically valid and current" : "Not valid/current"],
    ["Revocation", decision.revocation_status],
    ["Digest scope", shortDigest(capsule.application_digest)],
    ...(recovery ? [["Recovery journal SHA-256", recovery.rollback_journal_sha256_before]] : []),
  ]);
  renderCapabilities(capsule);
  forgetDecisionButton.disabled = !["denied_by_user", "locally_trusted_exact_release"].includes(decision.trust_state);
  forgetDecisionButton.title = forgetDecisionButton.disabled
    ? "No exact current file or release decision is active."
    : "Remove only the exact current file/release decision and grants. Publisher trust, revocations, backups, other capsules, and audit history remain.";
  revokeButton.disabled = !decision.publisher_trusted || !decision.signature_valid;
  revokeButton.title = revokeButton.disabled
    ? "Only a locally trusted current publisher key can be revoked here."
    : "Revocation requires typing the exact key fingerprint.";
  alwaysButton.disabled = !decision.signature_valid;
  alwaysButton.title = decision.signature_valid
    ? "Persist this exact signed application digest and the selected grants."
    : "Always is available only for a currently valid signed application.";
  actions.querySelectorAll("button:not(#always-button)").forEach((button) => { button.disabled = false; });
  boundaryTitle.textContent = authorized
    ? capsule.assets_released
      ? "Application window · verified assets · exact named bridge"
      : "Application window · release gate passed, payload locked"
    : "Application window · executable assets locked";
  if (report.stage === "first-open" && lastFocusKey !== focusKey) {
    promptTitle.focus();
  }
  lastFocusKey = focusKey;
}

function selectedCapabilities() {
  return [...capabilityList.querySelectorAll("input:checked")].map((input) => input.value);
}

async function invokeHost(command, payload = {}) {
  const invoke = globalThis.__TAURI__?.core?.invoke;
  if (typeof invoke !== "function") throw new Error("The trusted Tauri API was not injected.");
  return invoke(command, payload);
}

actions.addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-action]");
  if (!button || button.disabled) return;
  actions.querySelectorAll("button").forEach((item) => { item.disabled = true; });
  actionStatus.textContent = "Applying the host-local decision…";
  try {
    const report = await invokeHost("first_open_decide", {
      request: { action: button.dataset.action, capabilities: selectedCapabilities() },
    });
    renderReport(report);
    actionStatus.textContent = button.dataset.action === "cancel"
      ? "Cancelled. No trust or capability decision was changed."
      : "Decision applied. The effective policy is shown above.";
  } catch (error) {
    actionStatus.textContent = String(error);
    actionStatus.className = "action-status error";
    actions.querySelectorAll("button").forEach((item) => { item.disabled = false; });
  }
});

document.querySelector(".admin-actions").addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-admin-action]");
  if (!button || button.disabled) return;
  const action = button.dataset.adminAction;
  let confirmation = null;
  if (action === "forget_current_decision") {
    confirmation = globalThis.prompt("This removes only the exact current file/release decision and its grants. It grants no authority and preserves other trust records. Type FORGET-CURRENT-DECISION to continue:") || "";
    if (!confirmation) return;
  } else if (action === "revoke_current_key") {
    confirmation = globalThis.prompt("Type the exact current key fingerprint to revoke it:") || "";
    if (!confirmation) return;
  } else if (action === "reset") {
    confirmation = globalThis.prompt("A verified host-local backup will be created first. Type ERASE-TRUST-DECISIONS to reset local decisions:") || "";
    if (!confirmation) return;
  }
  adminOutput.textContent = "Reading the protected host-local record…";
  try {
    const response = await invokeHost("trust_admin", {
      request: { action, confirmation },
    });
    renderReport(response.report);
    adminOutput.textContent = JSON.stringify(response.output, null, 2);
  } catch (error) {
    adminOutput.textContent = `Action rejected: ${String(error)}`;
  }
});

invokeHost("startup_report")
  .then(renderReport)
  .catch((error) => {
    renderReport({ stage: "host-api", capsule: null, error: String(error) });
  });

invokeHost("update_status")
  .then(renderUpdateStatus)
  .catch((error) => {
    updateStatus.textContent = `Host update status unavailable: ${String(error)}`;
    updateStatus.className = "error";
  });

updateCheckButton.addEventListener("click", async () => {
  if (updateCheckButton.disabled) return;
  updateCheckButton.disabled = true;
  updateStatus.className = "";
  updateStatus.textContent = "Checking the compiled HTTPS endpoint for host-update metadata…";
  try {
    const report = await invokeHost("check_host_update");
    reviewedUpdateVersion = report.available && report.release_policy_verified
      ? report.candidate_version
      : null;
    updateDownloadControls.hidden = !reviewedUpdateVersion;
    updateDownloadConsent.checked = false;
    updateDownloadButton.disabled = true;
    updateStageControls.hidden = true;
    updateStageConsent.checked = false;
    updateStageButton.disabled = true;
    updateValidationStatus.textContent = report.release_policy_verified
      ? `Release sequence ${report.candidate_sequence} passed the compiled Ed25519 release policy. The exact artifact URL, target, version, origin, and platform-signing class match.`
      : "";
    updateStatus.textContent = report.available
      ? `Host ${report.current_version} · signed policy verified ${report.candidate_version} for ${report.target}. Nothing was downloaded, staged, authorized, or installed.`
      : `Host ${report.current_version} · no newer host update was announced by ${report.transport_endpoint_origin}.`;
  } catch (error) {
    updateStatus.textContent = `Host update check failed closed: ${String(error)}`;
    updateStatus.className = "error";
  } finally {
    updateCheckButton.disabled = false;
  }
});

updateDownloadConsent.addEventListener("change", () => {
  updateDownloadButton.disabled = !updateDownloadConsent.checked || !reviewedUpdateVersion;
});

updateDownloadButton.addEventListener("click", async () => {
  if (updateDownloadButton.disabled || !reviewedUpdateVersion) return;
  updateDownloadButton.disabled = true;
  updateCheckButton.disabled = true;
  updateValidationStatus.className = "lifecycle-action-status";
  updateValidationStatus.textContent = "Downloading the bounded package and Sigstore evidence from the compiled origin…";
  try {
    const report = await invokeHost("download_host_update", {
      request: {
        candidate_version: reviewedUpdateVersion,
        confirmation: "DOWNLOAD HOST UPDATE",
      },
    });
    updateDownloadControls.hidden = true;
    updateStageControls.hidden = false;
    updateStageConsent.checked = false;
    updateStageButton.disabled = true;
    updateValidationStatus.textContent = `Downloaded ${report.artifact_bytes.toLocaleString()} package bytes and ${report.sigstore_bundle_bytes.toLocaleString()} Sigstore-evidence bytes. Minisign, signed release digests, the pinned platform signer, required timestamp, Fulcio chain/SCT, Rekor proof, exact Sigstore identity, and OIDC issuer all match.`;
    updateStatus.textContent = `Verified host update ${report.candidate_version} is held only in protected host memory. It is not staged, authorized, or installed.`;
  } catch (error) {
    updateValidationStatus.textContent = `Host update download failed closed: ${String(error)}`;
    updateValidationStatus.className = "lifecycle-action-status error";
    updateDownloadButton.disabled = !updateDownloadConsent.checked;
  } finally {
    updateCheckButton.disabled = false;
  }
});

updateStageConsent.addEventListener("change", () => {
  updateStageButton.disabled = !updateStageConsent.checked || !reviewedUpdateVersion;
});

updateStageButton.addEventListener("click", async () => {
  if (updateStageButton.disabled || !reviewedUpdateVersion) return;
  updateStageButton.disabled = true;
  updateCheckButton.disabled = true;
  updateValidationStatus.className = "lifecycle-action-status";
  updateValidationStatus.textContent = "Establishing the capsule recovery point, closing the active session, and staging the exact verified update bytes…";
  try {
    const report = await invokeHost("stage_host_update", {
      request: {
        candidate_version: reviewedUpdateVersion,
        confirmation: "INSTALL HOST UPDATE",
      },
    });
    updateStageControls.hidden = true;
    reviewedUpdateVersion = null;
    const backup = report.preflight.verified_backup
      ? " A verified current-state capsule backup was recorded."
      : " No writable capsule state required a new backup.";
    updateValidationStatus.textContent = `Stage ${report.stage_id} contains ${report.artifact_name} and ${report.sigstore_name}.${backup}`;
    updateStatus.textContent = `Verified host update ${report.candidate_version} is install-authorized and durably staged, but the installer has not been run.`;
  } catch (error) {
    updateValidationStatus.textContent = `Host update staging failed closed: ${String(error)}`;
    updateValidationStatus.className = "lifecycle-action-status error";
    updateStageButton.disabled = !updateStageConsent.checked;
  } finally {
    updateCheckButton.disabled = false;
  }
});

const listen = globalThis.__TAURI__?.event?.listen;
if (typeof listen === "function") {
  listen("host-report", (event) => renderReport(event.payload)).catch((error) => {
    adminOutput.textContent = `Host event channel unavailable: ${String(error)}`;
  });
  listen("host-message", (event) => {
    if (event.payload.kind.startsWith("support-")) {
      adminOutput.textContent = event.payload.message;
      return;
    }
    const restoreMessage = event.payload.kind.startsWith("restore-");
    const target = restoreMessage ? lifecycleActionStatus : actionStatus;
    const baseClass = restoreMessage ? "lifecycle-action-status" : "action-status";
    target.textContent = event.payload.message;
    target.className = event.payload.kind.endsWith("error") ? `${baseClass} error` : baseClass;
  }).catch((error) => {
    adminOutput.textContent = `Host message channel unavailable: ${String(error)}`;
  });
  listen("restore-report", (event) => {
    const report = event.payload;
    lifecycleActionStatus.textContent = `Verified copy restored to ${report.restored_path} (${report.record.output_bytes.toLocaleString()} bytes, ${shortDigest(report.record.output_sha256)}). Review the restored capsule before running it.`;
    lifecycleActionStatus.className = "lifecycle-action-status";
  }).catch((error) => {
    lifecycleActionStatus.textContent = `Restore report unavailable: ${String(error)}`;
    lifecycleActionStatus.className = "lifecycle-action-status error";
  });
}

openButton.addEventListener("click", async () => {
  openButton.disabled = true;
  actionStatus.textContent = "Choose one local SQLite Capsule. Its content will be inspected before any code runs.";
  actionStatus.className = "action-status";
  try {
    await invokeHost("open_capsule_picker");
  } catch (error) {
    actionStatus.textContent = String(error);
    actionStatus.className = "action-status error";
  } finally {
    openButton.disabled = false;
  }
});

reopenButton.addEventListener("click", async () => {
  reopenButton.disabled = true;
  lifecycleActionStatus.textContent = "Re-inspecting the current path. Application code remains locked.";
  lifecycleActionStatus.className = "lifecycle-action-status";
  try {
    renderReport(await invokeHost("reopen_current_capsule"));
  } catch (error) {
    lifecycleActionStatus.textContent = String(error);
    lifecycleActionStatus.className = "lifecycle-action-status error";
  }
});

readOnlyButton.addEventListener("click", async () => {
  readOnlyButton.disabled = true;
  lifecycleActionStatus.textContent = "Re-inspecting and opening without source write authority.";
  lifecycleActionStatus.className = "lifecycle-action-status";
  try {
    const report = await invokeHost("continue_current_read_only");
    renderReport(report);
    if (!report.capsule?.assets_released) {
      lifecycleActionStatus.textContent = "This file cannot prove the same signed application compartment. Review and authorize it again before any code is released; the prior verified backup remains available.";
    }
  } catch (error) {
    lifecycleActionStatus.textContent = String(error);
    lifecycleActionStatus.className = "lifecycle-action-status error";
  }
});

restoreButton.addEventListener("click", async () => {
  if (!currentBackupId || restoreButton.disabled) return;
  restoreButton.disabled = true;
  lifecycleActionStatus.textContent = "Choose a new path. Existing files are never replaced.";
  lifecycleActionStatus.className = "lifecycle-action-status";
  try {
    await invokeHost("restore_backup_picker", { request: { backup_id: currentBackupId } });
  } catch (error) {
    lifecycleActionStatus.textContent = String(error);
    lifecycleActionStatus.className = "lifecycle-action-status error";
  }
});

supportButton.addEventListener("click", async () => {
  supportButton.disabled = true;
  adminOutput.textContent = "Preparing a redacted host-owned support bundle…";
  try {
    await invokeHost("export_support_bundle_picker");
  } catch (error) {
    adminOutput.textContent = `Support export rejected: ${String(error)}`;
  } finally {
    supportButton.disabled = false;
  }
});

async function refreshLifecycleStatus() {
  try {
    const status = await invokeHost("lifecycle_status");
    currentBackupId = status.backup?.backup_id || null;
    restoreButton.disabled = !currentBackupId;
    if (status.mode === "conflict_closed") {
      state.textContent = "Session conflict · renderer locked";
      boundaryTitle.textContent = "Application window · conflict closed · executable assets locked";
      lifecycleStatus.textContent = "Session closed because the source was replaced or changed externally. Reopen it, continue from an explicit read-only open, or restore a verified copy; no silent merge occurred.";
    } else if (!status.active) {
      lifecycleStatus.textContent = "Runtime locked. No source write is possible.";
    } else if (status.mode === "read_only_writer_busy") {
      lifecycleStatus.textContent = "Read-only session because another host owns the writer lease. The source cannot be changed here.";
    } else if (status.mode === "read_only_unsafe_filesystem") {
      lifecycleStatus.textContent = "Read-only session because Windows could not establish a safe fixed local filesystem. Use Save a verified copy before editing.";
    } else if (!status.writable) {
      lifecycleStatus.textContent = "Read-only session. The source cannot be changed.";
    } else if (!status.backup) {
      lifecycleStatus.textContent = "Writable session pinned. A verified backup is required before the first named write.";
    } else {
      lifecycleStatus.textContent = `Writable session pinned. Verified pre-write backup ${shortDigest(status.backup.sha256)} · ${status.backup.bytes.toLocaleString()} bytes.`;
    }
    const inventory = status.backup_inventory || {};
    const incomplete = inventory.incomplete_artifacts?.length || 0;
    const invalid = inventory.invalid_artifacts?.length || 0;
    if (incomplete || invalid) {
      lifecycleStatus.textContent += ` Recovery inventory requires attention: ${incomplete} interrupted and ${invalid} invalid artifact${incomplete + invalid === 1 ? "" : "s"}; none is treated as recoverable.`;
    }
  } catch (error) {
    currentBackupId = null;
    restoreButton.disabled = true;
    lifecycleStatus.textContent = `Lifecycle status unavailable: ${String(error)}`;
  }
}

refreshLifecycleStatus();
globalThis.setInterval(refreshLifecycleStatus, 2000);
