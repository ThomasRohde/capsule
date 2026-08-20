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
const openStatus = document.querySelector("#open-status");
const cabinetOpenButton = document.querySelector("#cabinet-open-button");
const cabinetOpenStatus = document.querySelector("#cabinet-open-status");
const recentCapsules = document.querySelector("#recent-capsules");
const copyStatus = document.querySelector("#copy-status");
const copyBadge = document.querySelector("#copy-badge");
const copyModeGrid = document.querySelector("#copy-mode-grid");
const copyProfileReview = document.querySelector("#copy-profile-review");
const copyProfileDatasets = document.querySelector("#copy-profile-datasets");
const copyDestinationStatus = document.querySelector("#copy-destination-status");
const copyDestinationButton = document.querySelector("#copy-destination-button");
const copyClearButton = document.querySelector("#copy-clear-button");
const copyPrepareButton = document.querySelector("#copy-prepare-button");
const copyReview = document.querySelector("#copy-review");
const copyReviewDetails = document.querySelector("#copy-review-details");
const copyDatasetReview = document.querySelector("#copy-dataset-review");
const copyConsent = document.querySelector("#copy-consent");
const copyConfirmation = document.querySelector("#copy-confirmation");
const copyExecuteButton = document.querySelector("#copy-execute-button");
const copyActionStatus = document.querySelector("#copy-action-status");
const copyResultWrap = document.querySelector("#copy-result-wrap");
const copyResult = document.querySelector("#copy-result");
const compareStatus = document.querySelector("#compare-status");
const compareBadge = document.querySelector("#compare-badge");
const compareChooseButton = document.querySelector("#compare-choose-button");
const compareCloseButton = document.querySelector("#compare-close-button");
const compareActionStatus = document.querySelector("#compare-action-status");
const compareReport = document.querySelector("#compare-report");
const compareCompatibilityTitle = document.querySelector("#compare-compatibility-title");
const compareCompatibilityReason = document.querySelector("#compare-compatibility-reason");
const compareCompatibilityBadge = document.querySelector("#compare-compatibility-badge");
const comparePairDetails = document.querySelector("#compare-pair-details");
const compareLimitBadge = document.querySelector("#compare-limit-badge");
const compareApplicationButton = document.querySelector("#compare-application-button");
const compareApplicationDetail = document.querySelector("#compare-application-detail");
const compareDatasets = document.querySelector("#compare-datasets");
const compareDetail = document.querySelector("#compare-detail");
const compareDetailTitle = document.querySelector("#compare-detail-title");
const compareDetailDescription = document.querySelector("#compare-detail-description");
const compareDetailBadge = document.querySelector("#compare-detail-badge");
const compareSensitiveConsent = document.querySelector("#compare-sensitive-consent");
const compareRevealButton = document.querySelector("#compare-reveal-button");
const compareDetailTableWrap = document.querySelector("#compare-detail-table-wrap");
const compareDetailRows = document.querySelector("#compare-detail-rows");
const compareNextButton = document.querySelector("#compare-next-button");
const reconcileBadge = document.querySelector("#reconcile-badge");
const reconcileOpenButton = document.querySelector("#reconcile-open-button");
const reconcileStatus = document.querySelector("#reconcile-status");
const reconcileOptionsWrap = document.querySelector("#reconcile-options");
const reconcileOrientations = document.querySelector("#reconcile-orientations");
const reconcileStartButton = document.querySelector("#reconcile-start-button");
const reconcileSessionWrap = document.querySelector("#reconcile-session");
const reconcileIdentity = document.querySelector("#reconcile-identity");
const reconcileSelections = document.querySelector("#reconcile-selections");
const reconcileConflicts = document.querySelector("#reconcile-conflicts");
const reconcileAncestorButton = document.querySelector("#reconcile-ancestor-button");
const reconcileThreeWayWrap = document.querySelector("#reconcile-three-way");
const reconcileSessionChecks = document.querySelector("#reconcile-session-checks");
const reconcileDestinationButton = document.querySelector("#reconcile-destination-button");
const reconcilePrepareButton = document.querySelector("#reconcile-prepare-button");
const reconcileDestinationStatus = document.querySelector("#reconcile-destination-status");
const reconcileReviewWrap = document.querySelector("#reconcile-review");
const reconcileReviewTitle = document.querySelector("#reconcile-review-title");
const reconcileReviewIdentity = document.querySelector("#reconcile-review-identity");
const reconcileOperationList = document.querySelector("#reconcile-operation-list");
const reconcileReviewChecks = document.querySelector("#reconcile-review-checks");
const reconcileConfirmation = document.querySelector("#reconcile-confirmation");
const reconcileExecuteButton = document.querySelector("#reconcile-execute-button");
const reconcileCancelButton = document.querySelector("#reconcile-cancel-button");
const reconcileResult = document.querySelector("#reconcile-result");
const reconcileResultOutput = document.querySelector("#reconcile-result-output");
const upgradeStatus = document.querySelector("#upgrade-status");
const upgradeBadge = document.querySelector("#upgrade-badge");
const upgradeReleaseStatus = document.querySelector("#upgrade-release-status");
const upgradeReleaseButton = document.querySelector("#upgrade-release-button");
const upgradeDestinationStatus = document.querySelector("#upgrade-destination-status");
const upgradeDestinationButton = document.querySelector("#upgrade-destination-button");
const upgradeCandidateDetails = document.querySelector("#upgrade-candidate-details");
const upgradePrepareButton = document.querySelector("#upgrade-prepare-button");
const upgradeReview = document.querySelector("#upgrade-review");
const upgradeReviewTitle = document.querySelector("#upgrade-review-title");
const upgradeReviewDetails = document.querySelector("#upgrade-review-details");
const upgradeCapabilities = document.querySelector("#upgrade-capabilities");
const upgradeDatasets = document.querySelector("#upgrade-datasets");
const upgradeChecks = document.querySelector("#upgrade-checks");
const upgradePublisherConfirmation = document.querySelector("#upgrade-publisher-confirmation");
const upgradeCapabilityConfirmationWrap = document.querySelector("#upgrade-capability-confirmation-wrap");
const upgradeCapabilityConfirmation = document.querySelector("#upgrade-capability-confirmation");
const upgradeExecuteButton = document.querySelector("#upgrade-execute-button");
const upgradeCancelButton = document.querySelector("#upgrade-cancel-button");
const upgradeActionStatus = document.querySelector("#upgrade-action-status");
const upgradeResult = document.querySelector("#upgrade-result");
const upgradeResultOutput = document.querySelector("#upgrade-result-output");
const reviewCapabilitiesButton = document.querySelector("#review-capabilities-button");
const overviewTitle = document.querySelector("#overview-title");
const overviewDescription = document.querySelector("#overview-description");
const overviewIconImage = document.querySelector("#overview-icon-image");
const overviewIconFallback = document.querySelector("#overview-icon-fallback");
const formatBadge = document.querySelector("#format-badge");
const profileBadge = document.querySelector("#profile-badge");
const applicationIdentity = document.querySelector("#application-identity");
const instanceIdentity = document.querySelector("#instance-identity");
const applicationStateBadge = document.querySelector("#application-state-badge");
const recoverSelectedButton = document.querySelector("#recover-selected-button");
const reopenButton = document.querySelector("#reopen-button");
const readOnlyButton = document.querySelector("#read-only-button");
const restoreButton = document.querySelector("#restore-button");
const signingStatus = document.querySelector("#signing-status");
const signingBadge = document.querySelector("#signing-badge");
const signingNavTag = document.querySelector("#signing-nav-tag");
const signingKeyStatus = document.querySelector("#signing-key-status");
const signingSourceStatus = document.querySelector("#signing-source-status");
const signingOutputStatus = document.querySelector("#signing-output-status");
const signingKeyButton = document.querySelector("#signing-key-button");
const signingSourceButton = document.querySelector("#signing-source-button");
const signingOutputButton = document.querySelector("#signing-output-button");
const signingPublisherId = document.querySelector("#signing-publisher-id");
const signingPublisherName = document.querySelector("#signing-publisher-name");
const signingClearButton = document.querySelector("#signing-clear-button");
const signingPrepareButton = document.querySelector("#signing-prepare-button");
const signingReview = document.querySelector("#signing-review");
const signingPreviewDetails = document.querySelector("#signing-preview-details");
const signingConsent = document.querySelector("#signing-consent");
const signingConfirmation = document.querySelector("#signing-confirmation");
const signingExecuteButton = document.querySelector("#signing-execute-button");
const signingActionStatus = document.querySelector("#signing-action-status");
const signingResultWrap = document.querySelector("#signing-result-wrap");
const signingResult = document.querySelector("#signing-result");
const promptTitle = document.querySelector("#prompt-title");
const pageTitle = document.querySelector("#page-title");
const pageSubtitle = document.querySelector("#page-subtitle");
const pageContent = document.querySelector("#page-content");
const navSearch = document.querySelector("#nav-search");
const overviewNavTag = document.querySelector("#overview-nav-tag");
const updateNavTag = document.querySelector("#update-nav-tag");
const themeQuery = globalThis.matchMedia("(prefers-color-scheme: dark)");
let currentBackupId = null;
let lastFocusKey = null;
let reviewedUpdateVersion = null;
let signingSession = null;
let copyDestination = null;
let copyProfilePreview = null;
let preparedCopy = null;
let activeCopyOperationId = null;
let compareSession = null;
let compareTableSelection = null;
let compareNextPageToken = null;
let comparePageRevealed = false;
let reconcileOptions = null;
let reconcileSession = null;
let reconcileDestination = null;
let reconcileThreeWay = null;
const reconcileFinalizations = new Map();
let preparedReconcile = null;
let activeReconcileOperationToken = null;
let upgradeCandidate = null;
let upgradeDestination = null;
let preparedUpgrade = null;
let activeUpgradeOperationToken = null;
const upgradeFinalizations = new Map();
let currentReport = null;
let selectedTheme = "dark";

const pageCopy = {
  cabinet: ["Cabinet", "Choose a capsule to inspect. Recent metadata is a convenience only and never execution authority."],
  overview: ["Overview", "Bounded host-owned application, capsule, publisher, and file identity before execution."],
  copy: ["Create copy", "Review one create-new operation. Paths, retained sources, plans, and publication authority remain in Rust."],
  lineage: ["Lineage", "Mutable provenance is visibly separate from publisher authentication."],
  compare: ["Compare", "Read-only compatibility, identity, lineage, application, schema, and signed-policy data comparison."],
  versions: ["Versions", "Install a strictly newer same-schema signed application release into a verified new copy."],
  security: ["Security", "Trust, capabilities, publisher tools, local trust, and the application boundary."],
  capabilities: ["Capabilities", "Host-owned prompt. Capabilities come from the verified manifest."],
  protection: ["Data protection", "Session mode, verified backups, and recovery for the open capsule."],
  signing: ["Publisher signing", "Host-owned, use-once signing of a verified capsule copy. Private key bytes never enter JavaScript."],
  updates: ["Settings", "Compiled, pinned host-only updater. Nothing downloads, stages, or installs without consent."],
  admin: ["Local trust controls", "Host-local decision record. Capsule-controlled labels remain untrusted text."],
  boundary: ["Application window", "The untrusted renderer runs in a separate native window behind the named bridge."],
};

function selectPage(page, options = {}) {
  if (!pageCopy[page]) return;
  document.querySelectorAll("[data-page-panel]").forEach((panel) => {
    const active = panel.dataset.pagePanel === page;
    panel.hidden = !active;
    panel.classList.toggle("is-active", active);
  });
  const topPage = ["capabilities", "signing", "admin", "boundary"].includes(page) ? "security" : page;
  document.querySelectorAll(".nav-item[data-page]").forEach((button) => {
    const active = button.dataset.page === topPage;
    button.classList.toggle("is-selected", active);
    if (active) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  });
  const [title, subtitle] = pageCopy[page];
  pageTitle.textContent = title;
  pageSubtitle.textContent = subtitle;
  document.title = `SQLite Capsule Host — ${title.toLowerCase()}`;
  if (options.focus !== false) pageContent.focus();
}

function resolveTheme(mode) {
  return mode === "system" ? (themeQuery.matches ? "dark" : "light") : mode;
}

function applyTheme(mode) {
  selectedTheme = ["light", "dark", "system"].includes(mode) ? mode : "dark";
  document.documentElement.dataset.theme = resolveTheme(selectedTheme);
  document.querySelectorAll("[data-theme-option]").forEach((button) => {
    button.setAttribute("aria-pressed", String(button.dataset.themeOption === selectedTheme));
  });
  try { globalThis.localStorage.setItem("sqlite-capsule-theme", selectedTheme); } catch (_) { /* protected storage may be unavailable */ }
}

function setOverviewNavState(label, tone) {
  overviewNavTag.textContent = label;
  overviewNavTag.dataset.tone = tone;
}

document.querySelector(".page-navigation").addEventListener("click", (event) => {
  const button = event.target.closest(".nav-item[data-page]");
  if (button) selectPage(button.dataset.page);
});

document.querySelector("#page-content").addEventListener("click", (event) => {
  const route = event.target.closest("button[data-route]")?.dataset.route;
  if (route) selectPage(route);
});

navSearch.addEventListener("input", () => {
  const query = navSearch.value.trim().toLocaleLowerCase();
  document.querySelectorAll(".nav-item[data-page]").forEach((button) => {
    button.hidden = Boolean(query) && !button.textContent.toLocaleLowerCase().includes(query);
  });
});

document.querySelector(".theme-options").addEventListener("click", (event) => {
  const button = event.target.closest("button[data-theme-option]");
  if (button) applyTheme(button.dataset.themeOption);
});

themeQuery.addEventListener("change", () => {
  if (selectedTheme === "system") applyTheme("system");
});

try { selectedTheme = globalThis.localStorage.getItem("sqlite-capsule-theme") || "dark"; } catch (_) { selectedTheme = "dark"; }
applyTheme(selectedTheme);

function hostWindow() {
  return globalThis.__TAURI__?.window?.getCurrentWindow?.() || null;
}

document.querySelector("#window-minimize").addEventListener("click", () => { hostWindow()?.minimize(); });
document.querySelector("#window-maximize").addEventListener("click", () => { hostWindow()?.toggleMaximize(); });
document.querySelector("#window-close").addEventListener("click", () => { hostWindow()?.close(); });

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
  updateNavTag.hidden = !report.release_policy_verified && !report.state && !report.error && !attention;
  updateNavTag.textContent = report.error || attention ? "!" : report.release_policy_verified ? "1" : "•";
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

function hostError(error) {
  if (error && typeof error === "object" && typeof error.safe_detail === "string") {
    return error.safe_detail;
  }
  return String(error);
}

function signingPublisherValuesMatchPreview() {
  const preview = signingSession?.preview;
  return Boolean(preview)
    && signingPublisherId.value === preview.publisher_id
    && signingPublisherName.value === preview.publisher_name;
}

function refreshSigningActions() {
  const busy = Boolean(signingSession?.busy);
  const publisherReady = signingPublisherId.value.length > 0 && signingPublisherName.value.length > 0;
  const selectionsReady = Boolean(signingSession?.key && signingSession?.source && signingSession?.output);
  const reviewed = signingPublisherValuesMatchPreview();
  signingPrepareButton.disabled = busy || !publisherReady || !selectionsReady || reviewed;
  signingConfirmation.disabled = busy || !reviewed;
  if (!reviewed) signingConfirmation.checked = false;
  signingExecuteButton.disabled = busy || !reviewed || !signingConfirmation.checked;
  signingConsent.hidden = !reviewed;
}

function renderSigningSession(report) {
  signingSession = report;
  const busy = Boolean(report.busy);
  signingKeyButton.disabled = busy;
  signingSourceButton.disabled = busy;
  signingOutputButton.disabled = busy || !report.source;
  signingClearButton.disabled = busy || !(report.key || report.source || report.output || report.preview);
  signingPublisherId.disabled = busy;
  signingPublisherName.disabled = busy;

  if (report.key) {
    signingKeyStatus.textContent = `${report.key.file_name} · ${report.key.format} · ${report.key.key_id}`;
  } else {
    signingKeyStatus.textContent = "Select a 32-byte seed, 64-digit hex seed, or Ed25519 PKCS#8 PEM/DER file.";
  }
  signingSourceStatus.textContent = report.source
    ? `${report.source.path} · ${report.source.bytes.toLocaleString()} bytes · SHA-256 ${shortDigest(report.source.sha256)}`
    : "The source is inspected and verified before any signing copy is prepared.";
  signingOutputStatus.textContent = report.output
    ? `${report.output} · new file only`
    : "Existing files and the source path are never replaced.";

  signingBadge.className = `badge ${report.preview ? "ok" : report.key ? "warn" : ""}`.trim();
  signingBadge.textContent = report.preview ? "Ready to sign" : report.key ? "Key loaded" : "No key";
  if (signingNavTag) signingNavTag.textContent = report.preview ? "Ready" : report.key ? "Key" : "Use once";
  signingStatus.textContent = busy
    ? "Protected host operation in progress. No capsule application code is executing."
    : report.preview
      ? "The exact copy and application digest are prepared for final review. The source remains unchanged."
      : report.key
        ? "A use-once private key is held in Rust memory. It will be consumed by signing or forgotten when this session is cleared."
        : "No private key is loaded. Signing keys are use-once and remain in Rust memory only.";

  if (report.preview) {
    const preview = report.preview;
    signingReview.hidden = false;
    signingReview.open = true;
    setRows(signingPreviewDetails, [
      ["Source", preview.source.path],
      ["Source SHA-256", preview.source.sha256],
      ["Application digest", preview.application_digest],
      ["Key fingerprint", report.key?.key_id || "Key unavailable"],
      ["Publisher ID", preview.publisher_id],
      ["Publisher name", preview.publisher_name],
      ["Signed at", preview.signed_at],
      ["New output", preview.output],
    ]);
  } else {
    signingReview.hidden = true;
    signingReview.open = false;
    signingPreviewDetails.replaceChildren();
  }
  refreshSigningActions();
}

function renderCapabilities(capsule) {
  const decisions = capsule.decision.capabilities || {};
  const rows = Object.entries(decisions).map(([name, evaluation]) => {
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
    reason.textContent = evaluation.reason;
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

function renderCabinet(snapshot) {
  const entries = Array.isArray(snapshot?.entries) ? snapshot.entries.slice(0, 12) : [];
  if (!entries.length) {
    const empty = document.createElement("li");
    empty.textContent = "No recent capsules yet.";
    recentCapsules.replaceChildren(empty);
    return;
  }
  recentCapsules.replaceChildren(...entries.map((entry) => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    const title = document.createElement("strong");
    const detail = document.createElement("span");
    title.textContent = entry.instance_title || entry.application_name || "Unnamed capsule";
    detail.textContent = `v${entry.format_version || "?"} · ${entry.last_observed_badge || "Reinspect required"} · ${entry.last_opened_at || "Unknown time"}`;
    button.type = "button";
    button.className = "recent-capsule-button";
    button.append(title, detail);
    button.addEventListener("click", async () => {
      button.disabled = true;
      cabinetOpenStatus.textContent = "Re-inspecting the selected source. Cached identity and trust are not reused.";
      cabinetOpenStatus.className = "action-status";
      try {
        renderReport(await invokeHost("open_recent_capsule", { recentId: entry.recent_id }));
      } catch (error) {
        cabinetOpenStatus.textContent = `Recent open failed closed: ${hostError(error)}`;
        cabinetOpenStatus.className = "action-status error";
        button.disabled = false;
      }
    });
    item.append(button);
    return item;
  }));
}

function initials(value) {
  const words = String(value || "SQLite Capsule").trim().split(/\s+/u).filter(Boolean);
  return words.slice(0, 2).map((word) => word[0]?.toLocaleUpperCase() || "").join("") || "SC";
}

function renderOverview(capsule) {
  const identity = capsule.identity;
  const overview = capsule.overview || identity.overview || {};
  const application = overview.application || {};
  const instance = overview.instance || null;
  const dataSchema = overview.data_schema || null;
  const legacy = overview.compatibility === "legacy-v02" || application.legacy_fallback === true || identity.format_version === "0.2";
  overviewTitle.textContent = instance?.title || application.name || identity.title || "Capsule overview";
  overviewDescription.textContent = instance?.description || application.description || identity.summary || "No description supplied.";
  formatBadge.textContent = `Format v${overview.format_version || identity.format_version}`;
  formatBadge.className = "badge";
  profileBadge.textContent = legacy ? "Legacy v0.2" : "Lifecycle v0.3";
  profileBadge.className = `badge ${legacy ? "warn" : "ok"}`;
  const publisherState = application.publisher?.state || (capsule.decision.signature_valid ? "signature-valid" : "unsigned");
  applicationStateBadge.textContent = publisherState === "signature-valid"
    ? "Signature valid"
    : publisherState === "invalid-signature"
      ? "Invalid signature"
      : "Unsigned";
  applicationStateBadge.className = `badge ${publisherState === "signature-valid" ? "ok" : publisherState === "invalid-signature" ? "fail" : "warn"}`;

  const safeIcon = application.safe_image || instance?.safe_image;
  const iconSource = safeIcon?.data_url;
  const allowedIcon = typeof iconSource === "string"
    && (iconSource.startsWith("data:image/png;base64,") || iconSource.startsWith("data:image/webp;base64,"));
  overviewIconImage.hidden = !allowedIcon;
  overviewIconFallback.hidden = allowedIcon;
  overviewIconImage.removeAttribute("src");
  if (allowedIcon) overviewIconImage.src = iconSource;
  overviewIconFallback.textContent = initials(application.name || instance?.title || identity.title);

  setRows(applicationIdentity, [
    [legacy ? "Legacy application" : "Application", application.name || identity.title],
    ["Application ID", application.app_id || identity.app_id],
    ["Release", application.app_version || identity.app_version],
    ["Category", application.category || (legacy ? "Legacy capsule" : "Not declared")],
    ["Released", application.released_at || "Not available in v0.2"],
    ["Publisher metadata", capsule.publisher?.name || application.publisher?.name || "None"],
    ["Cryptographic state", applicationStateBadge.textContent],
    ["Host trust", application.publisher?.host_trust || (capsule.decision.publisher_trusted ? "trusted" : "not trusted")],
    ["Revocation", application.publisher?.revocation || capsule.decision.revocation_status],
  ]);
  if (instance) {
    setRows(instanceIdentity, [
      ["Title", instance.title],
      ["Capsule ID", instance.capsule_id],
      ["Revision", instance.revision_id || "Not declared"],
      ["Document kind", instance.document_kind],
      ["Tags", Array.isArray(instance.tags) && instance.tags.length ? instance.tags.join(", ") : "None"],
      ["Updated", instance.content_updated_at],
      ["Data schema", dataSchema ? `${dataSchema.id || dataSchema.data_schema_id} v${dataSchema.version || dataSchema.data_schema_version}` : "Not declared"],
    ]);
  } else {
    setRows(instanceIdentity, [
      ["Profile", "Not available in legacy v0.2"],
      ["Compatibility", "No v0.3 revision, tags, or data-schema identity is synthesized"],
    ]);
  }
}

function renderReport(report) {
  const priorSelectionId = currentSelectionId();
  currentReport = report;
  const nextSelectionId = currentSelectionId();
  if (priorSelectionId && priorSelectionId !== nextSelectionId && !activeCopyOperationId) {
    resetCopyReview("The selected Capsule changed. Choose a fresh create-new destination.");
  }
  if (priorSelectionId && priorSelectionId !== nextSelectionId && compareSession) {
    resetCompareView("The selected Capsule changed. Choose a fresh comparison pair.");
  }
  if (priorSelectionId && priorSelectionId !== nextSelectionId) {
    if (!activeReconcileOperationToken) {
      resetReconcileView("The selected Capsule changed. Start a fresh comparison before reconciling.");
    } else {
      reconcileStatus.textContent = "The Cabinet selection changed. The host cancelled the prior reconciliation if publication was still cancellable.";
      reconcileStatus.className = "lifecycle-action-status error";
    }
  }
  if (priorSelectionId && priorSelectionId !== nextSelectionId) {
    if (!activeUpgradeOperationToken) {
      resetUpgradeView("The selected Capsule changed. Choose a fresh signed application release.");
    } else {
      upgradeStatus.textContent = "The Cabinet selection changed. The host cancelled the prior upgrade if publication was still cancellable.";
      upgradeStatus.className = "lifecycle-action-status error";
    }
  }
  const focusKey = focusKeyFor(report);
  void refreshCabinet();
  actionStatus.textContent = "";
  actionStatus.className = "action-status";
  openStatus.textContent = "";
  openStatus.className = "action-status";
  if (report.error) {
    state.textContent = "Rejected before execution";
    trustBadge.textContent = "Fail closed";
    trustBadge.className = "badge fail";
    setOverviewNavState("Blocked", "fail");
    setVerdict("Capsule rejected", report.error, "fail", "×");
    setRows(identityDetails, [["Stage", report.stage], ["Executable assets", "Not released"]]);
    capabilityList.replaceChildren();
    actions.querySelectorAll("button").forEach((button) => { button.disabled = true; });
    reviewCapabilitiesButton.disabled = true;
    recoverSelectedButton.disabled = report.stage !== "recovery-required";
    reopenButton.disabled = true;
    readOnlyButton.disabled = true;
    forgetDecisionButton.disabled = true;
    if (lastFocusKey !== focusKey) {
      selectPage(report.stage === "recovery-required" ? "protection" : "overview", { focus: false });
      (report.stage === "recovery-required" ? recoverSelectedButton : verdict).focus();
      lastFocusKey = focusKey;
    }
    return;
  }
  recoverSelectedButton.disabled = true;
  if (!report.capsule) {
    state.textContent = "No capsule selected";
    trustBadge.textContent = "Idle";
    trustBadge.className = "badge";
    setOverviewNavState("No file", "idle");
    setVerdict("Choose a capsule to begin", "Open with a .capsule.sqlite path. Nothing is executing.", "", "·");
    setRows(identityDetails, [["Stage", report.stage], ["Executable assets", "Not released"]]);
    capabilityList.replaceChildren();
    actions.querySelectorAll("button").forEach((button) => { button.disabled = true; });
    reviewCapabilitiesButton.disabled = true;
    reopenButton.disabled = true;
    readOnlyButton.disabled = true;
    forgetDecisionButton.disabled = true;
    if (lastFocusKey !== focusKey) {
      selectPage("cabinet", { focus: false });
      cabinetOpenButton.focus();
    }
    lastFocusKey = focusKey;
    return;
  }

  const capsule = report.capsule;
  const decision = capsule.decision;
  const identity = capsule.identity;
  const recovery = report.recovery;
  renderOverview(capsule);
  reviewCapabilitiesButton.disabled = false;
  const recoverySummary = recovery
    ? `SQLite recovery attempted · ${recovery.rollback_journal_hot_candidate_before ? "hot-journal candidate" : "rollback sidecar"} · ${recovery.rollback_journal_present_after ? "sidecar retained" : "sidecar cleared by SQLite"}`
    : "Not required";
  const label = trustLabels[decision.trust_state] || decision.trust_state;
  const blocked = ["unverified", "modified_after_signature", "invalid_signature", "denied_by_user", "revoked"].includes(decision.trust_state);
  const authorized = decision.executable_allowed;
  reviewCapabilitiesButton.textContent = authorized ? "Open application" : "Review capabilities";
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
  // Runtime authorization is not publisher authentication. An explicitly
  // allowed unsigned capsule stays visually cautionary even while running.
  trustBadge.className = `badge ${blocked ? "fail" : authorized && decision.signature_valid ? "ok" : "warn"}`;
  setOverviewNavState(blocked ? "Blocked" : authorized ? "Running" : "Ready", blocked ? "fail" : authorized ? "ok" : "warn");
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
  if (lastFocusKey !== focusKey) {
    selectPage("overview", { focus: false });
    overviewTitle.focus();
  }
  lastFocusKey = focusKey;
}

async function refreshCabinet() {
  try {
    renderCabinet(await invokeHost("cabinet_status"));
  } catch (_) {
    renderCabinet(null);
  }
}

function selectedCapabilities() {
  return [...capabilityList.querySelectorAll("input:checked")].map((input) => input.value);
}

async function invokeHost(command, payload = {}) {
  const invoke = globalThis.__TAURI__?.core?.invoke;
  if (typeof invoke !== "function") throw new Error("The trusted Tauri API was not injected.");
  return invoke(command, payload);
}

function compareLabel(value) {
  return String(value || "unavailable").replaceAll("-", " ");
}

function compareDisplayText(value) {
  return String(value ?? "").replace(/[\u0000-\u001f\u007f\u202a-\u202e\u2066-\u2069]/g, (character) => {
    const code = character.codePointAt(0).toString(16).toUpperCase().padStart(4, "0");
    return `\\u${code}`;
  });
}

function compareIsolate(value) {
  const isolate = document.createElement("bdi");
  isolate.textContent = compareDisplayText(value);
  return isolate;
}

function compareDefinition(label, value) {
  const dt = document.createElement("dt");
  const dd = document.createElement("dd");
  const isolate = document.createElement("bdi");
  dt.textContent = label;
  isolate.textContent = compareDisplayText(value);
  dd.append(isolate);
  return [dt, dd];
}

function resetReconcileView(message = "Open bounded row detail before applying changes. Only rows disclosed in this comparison can be selected.") {
  reconcileOptions = null;
  reconcileSession = null;
  reconcileDestination = null;
  reconcileThreeWay = null;
  preparedReconcile = null;
  if (!activeReconcileOperationToken) {
    reconcileOptionsWrap.hidden = true;
    reconcileSessionWrap.hidden = true;
    reconcileReviewWrap.hidden = true;
    reconcileResult.hidden = true;
    reconcileOrientations.replaceChildren(reconcileOrientations.querySelector("legend") || document.createElement("legend"));
    reconcileSelections.replaceChildren(reconcileSelections.querySelector("legend") || document.createElement("legend"));
    reconcileIdentity.replaceChildren();
    reconcileReviewIdentity.replaceChildren();
    reconcileOperationList.replaceChildren();
    reconcileSessionChecks.replaceChildren();
    reconcileReviewChecks.replaceChildren();
    reconcileThreeWayWrap.replaceChildren();
    reconcileThreeWayWrap.hidden = true;
    reconcileConfirmation.checked = false;
    reconcileExecuteButton.disabled = true;
    reconcileCancelButton.disabled = true;
    reconcilePrepareButton.disabled = true;
    reconcileAncestorButton.disabled = true;
    reconcileOpenButton.disabled = !compareSession;
    reconcileBadge.textContent = "No authority";
    reconcileBadge.className = "badge";
    reconcileStatus.textContent = message;
    reconcileStatus.className = "lifecycle-action-status";
  }
}

function resetCompareView(message = "Choose a second local Capsule to begin a bounded read-only comparison.") {
  compareSession = null;
  compareTableSelection = null;
  compareNextPageToken = null;
  comparePageRevealed = false;
  compareReport.hidden = true;
  compareDetail.hidden = true;
  compareSensitiveConsent.hidden = true;
  compareDetailTableWrap.hidden = true;
  compareDetailRows.replaceChildren();
  compareDatasets.replaceChildren();
  comparePairDetails.replaceChildren();
  compareApplicationDetail.replaceChildren();
  compareApplicationDetail.hidden = true;
  compareApplicationButton.disabled = true;
  compareChooseButton.disabled = false;
  compareCloseButton.disabled = true;
  compareNextButton.disabled = true;
  compareStatus.textContent = message;
  compareBadge.textContent = "No session";
  compareBadge.className = "badge";
  if (!activeReconcileOperationToken) resetReconcileView();
}

function renderCompareLayer(name, section) {
  const target = document.querySelector(`[data-compare-layer='${name}']`);
  if (!target) return;
  const count = Number(section?.change_count || 0);
  target.textContent = `${compareLabel(section?.state)} · ${count.toLocaleString()} bounded change${count === 1 ? "" : "s"}`;
  target.dataset.state = section?.state || "unavailable";
}

function compareCounts(summary) {
  const counts = summary?.counts;
  if (!counts || [counts.added, counts.removed, counts.changed, counts.unchanged].some((value) => value == null)) {
    return "Delta classification withheld by signed policy";
  }
  return `${Number(counts.added || 0).toLocaleString()} added · ${Number(counts.removed || 0).toLocaleString()} removed · ${Number(counts.changed || 0).toLocaleString()} changed · ${Number(counts.unchanged || 0).toLocaleString()} unchanged`;
}

function openCompareTable(dataset, table, selector) {
  compareTableSelection = {
    tableToken: selector.table_token,
    datasetLabel: dataset.dataset_id,
    tableLabel: table.table,
    sensitivity: dataset.sensitivity,
  };
  compareNextPageToken = null;
  comparePageRevealed = false;
  compareDetail.hidden = false;
  compareDetailTitle.replaceChildren(compareIsolate(dataset.dataset_id), document.createTextNode(" · "), compareIsolate(table.table));
  compareDetailDescription.textContent = `${compareLabel(dataset.policy)} policy · ${compareCounts(table)}`;
  compareDetailBadge.textContent = dataset.sensitivity;
  compareDetailBadge.className = `badge ${dataset.sensitivity === "sensitive" ? "warn" : ""}`;
  compareDetailRows.replaceChildren();
  compareDetailTableWrap.hidden = true;
  compareNextButton.disabled = true;
  if (dataset.sensitivity === "sensitive") {
    compareSensitiveConsent.hidden = false;
    compareActionStatus.textContent = "Sensitive values remain unavailable until the explicit reveal below.";
    compareDetailTitle.focus();
  } else {
    compareSensitiveConsent.hidden = true;
    void requestComparePage(false, null);
  }
}

function renderCompareDatasets(report, selectors) {
  compareDatasets.replaceChildren(...report.datasets.map((dataset, datasetIndex) => {
    const article = document.createElement("article");
    const heading = document.createElement("h3");
    const summary = document.createElement("p");
    const tableList = document.createElement("div");
    heading.append(compareIsolate(dataset.dataset_id));
    summary.textContent = `${dataset.sensitivity} · ${compareLabel(dataset.policy)} policy · ${compareLabel(dataset.state)} · ${Number(dataset.left_rows).toLocaleString()} left / ${Number(dataset.right_rows).toLocaleString()} right · ${compareCounts(dataset)}`;
    tableList.className = "compare-table-list";
    const selectorDataset = selectors[datasetIndex];
    dataset.tables.forEach((table, tableIndex) => {
      const row = document.createElement("div");
      const text = document.createElement("span");
      const selector = selectorDataset?.tables?.[tableIndex];
      text.append(
        compareIsolate(table.table),
        document.createTextNode(` · ${compareLabel(table.state)} · ${Number(table.left_rows).toLocaleString()} / ${Number(table.right_rows).toLocaleString()} rows · ${compareCounts(table)}`),
      );
      row.append(text);
      if (selector?.detail_available) {
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = dataset.sensitivity === "sensitive" ? "Review sensitive detail" : "Open bounded detail";
        button.addEventListener("click", () => openCompareTable(dataset, table, selector));
        row.append(button);
      } else {
        const unavailable = document.createElement("small");
        unavailable.textContent = dataset.policy === "ignore" || dataset.policy === "summary"
          ? "Signed policy permits summary only"
          : table.truncated ? "Detail limit reached" : "Detail unavailable";
        row.append(unavailable);
      }
      tableList.append(row);
    });
    article.append(heading, summary, tableList);
    return article;
  }));
  if (report.datasets.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = report.compatibility.can_compare_data
      ? "The signed contract declares no comparable datasets."
      : "Domain-data comparison is unavailable for this compatibility state.";
    compareDatasets.append(empty);
  }
}

function renderCompareSession(session) {
  if (session?.profile !== "org.sqlite-capsule.tauri-compare-session/1") {
    throw new Error("The host returned an unsupported comparison-session profile.");
  }
  compareSession = session;
  resetReconcileView();
  reconcileOpenButton.disabled = false;
  compareReport.hidden = false;
  compareDetail.hidden = true;
  compareCloseButton.disabled = false;
  compareApplicationButton.disabled = false;
  const report = session.report;
  const compatibility = report.compatibility;
  compareCompatibilityBadge.textContent = compareLabel(compatibility.state);
  compareCompatibilityBadge.className = `badge ${compatibility.can_compare_data ? "ok" : "warn"}`;
  compareCompatibilityReason.textContent = compatibility.reasons.join(" · ");
  comparePairDetails.replaceChildren(
    ...compareDefinition("Selected application", `${report.left.app_id} · ${report.left.app_version}`),
    ...compareDefinition("Comparison application", `${report.right.app_id} · ${report.right.app_version}`),
    ...compareDefinition("Selected instance", `${report.left.capsule_id} · revision ${report.left.revision_id}`),
    ...compareDefinition("Comparison instance", `${report.right.capsule_id} · revision ${report.right.revision_id}`),
    ...compareDefinition("Selected signature evidence", `${report.left.publisher.signature_count} valid envelope${report.left.publisher.signature_count === 1 ? "" : "s"} · ${report.left.publisher.publisher_name}`),
    ...compareDefinition("Comparison signature evidence", `${report.right.publisher.signature_count} valid envelope${report.right.publisher.signature_count === 1 ? "" : "s"} · ${report.right.publisher.publisher_name}`),
    ...compareDefinition("Report digest", report.report_digest),
  );
  ["identity", "lineage", "application", "schema"].forEach((name) => renderCompareLayer(name, report[name]));
  compareLimitBadge.textContent = report.truncated ? "Bounded · truncated" : "Bounded · complete";
  compareLimitBadge.className = `badge ${report.truncated ? "warn" : "ok"}`;
  renderCompareDatasets(report, session.selectors);
  compareStatus.textContent = `Comparison ready · ${compareLabel(compatibility.state)} · expires ${session.expires_at}`;
  compareBadge.textContent = "Read-only report";
  compareBadge.className = "badge ok";
  compareActionStatus.textContent = "Both retained sources were rechecked after comparison. No application assets or endpoints were executed.";
  compareActionStatus.className = "lifecycle-action-status";
  selectPage("compare", { focus: false });
  compareCompatibilityTitle.focus();
}

function renderCompareApplicationDetail(detail) {
  if (!compareSession || detail?.profile !== "org.sqlite-capsule.compare-application/1"
    || detail.comparison_report_digest !== compareSession.report.report_digest
    || detail.left_file_sha256 !== compareSession.report.left.file_sha256
    || detail.right_file_sha256 !== compareSession.report.right.file_sha256) {
    throw new Error("The host returned stale application-compartment evidence.");
  }
  compareApplicationDetail.replaceChildren(...detail.families.map((family) => {
    const row = document.createElement("div");
    const label = document.createElement("strong");
    const evidence = document.createElement("span");
    label.textContent = compareLabel(family.family);
    evidence.textContent = `${compareLabel(family.state)} · ${Number(family.table_count).toLocaleString()} table${family.table_count === 1 ? "" : "s"} · ${Number(family.left_rows).toLocaleString()} / ${Number(family.right_rows).toLocaleString()} rows · ${Number(family.change_count).toLocaleString()} bounded changes · ${shortDigest(family.left_digest)} / ${shortDigest(family.right_digest)}`;
    row.append(label, evidence);
    return row;
  }));
  compareApplicationDetail.hidden = false;
  compareApplicationButton.textContent = "Refresh bounded application families";
  compareActionStatus.textContent = `Application compartment rechecked · ${detail.families.length.toLocaleString()} fixed families · digest ${shortDigest(detail.detail_digest)}.`;
  compareActionStatus.className = "lifecycle-action-status";
}

compareApplicationButton.addEventListener("click", async () => {
  if (!compareSession) return;
  compareApplicationButton.disabled = true;
  compareActionStatus.textContent = "Re-verifying both retained sources and hashing fixed application-compartment families…";
  try {
    const detail = await invokeHost("get_compare_application_detail", {
      request: { session_id: compareSession.session_id },
    });
    renderCompareApplicationDetail(detail);
  } catch (error) {
    compareActionStatus.textContent = hostError(error);
    compareActionStatus.className = "lifecycle-action-status error";
  } finally {
    compareApplicationButton.disabled = false;
  }
});

function renderCompareValue(value) {
  const wrap = document.createElement("span");
  const display = document.createElement("bdi");
  wrap.className = "compare-value";
  if (!value) {
    display.textContent = "absent";
  } else if (value.storage_class === "blob") {
    display.textContent = `BLOB · ${Number(value.byte_count).toLocaleString()} bytes · sha256 ${shortDigest(value.sha256)}`;
  } else if (value.redacted || value.display == null) {
    display.textContent = `${value.storage_class} · redacted · sha256 ${shortDigest(value.sha256)}`;
  } else {
    display.textContent = compareDisplayText(value.display);
    if (value.truncated) display.append(document.createTextNode(" … [truncated]"));
  }
  wrap.append(display);
  return wrap;
}

function renderComparePage(page) {
  if (!compareSession || page?.profile !== "org.sqlite-capsule.compare-page/1"
    || page.session_id !== compareSession.session_id
    || page.report_digest !== compareSession.report.report_digest) {
    throw new Error("The host returned a stale comparison page.");
  }
  comparePageRevealed = Boolean(page.revealed);
  compareNextPageToken = page.next_page_token;
  compareSensitiveConsent.hidden = true;
  compareDetailTableWrap.hidden = false;
  compareDetailBadge.textContent = page.sensitivity === "sensitive" ? "Sensitive · revealed" : page.sensitivity;
  compareDetailRows.replaceChildren(...page.rows.map((row) => {
    const tr = document.createElement("tr");
    const kind = document.createElement("th");
    const evidence = document.createElement("td");
    const fields = document.createElement("td");
    kind.scope = "row";
    kind.textContent = row.kind;
    evidence.textContent = `key ${shortDigest(row.key_digest)} · left ${row.left_digest ? shortDigest(row.left_digest) : "absent"} · right ${row.right_digest ? shortDigest(row.right_digest) : "absent"}`;
    if (row.fields.length === 0) {
      fields.textContent = "Field values withheld by signed row policy";
    } else {
      const list = document.createElement("ul");
      row.fields.forEach((field) => {
        const item = document.createElement("li");
        const label = document.createElement("strong");
        label.append(compareIsolate(field.column), document.createTextNode(` · ${field.kind}: `));
        item.append(label, renderCompareValue(field.left), document.createTextNode(" → "), renderCompareValue(field.right));
        list.append(item);
      });
      fields.append(list);
    }
    tr.append(kind, evidence, fields);
    return tr;
  }));
  if (page.rows.length === 0) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = 3;
    td.textContent = "No row differences on this page.";
    tr.append(td);
    compareDetailRows.append(tr);
  }
  compareNextButton.disabled = !compareNextPageToken;
  compareActionStatus.textContent = `${page.rows.length.toLocaleString()} bounded row result${page.rows.length === 1 ? "" : "s"}. ${compareNextPageToken ? "A consumed continuation token is ready." : "No further page is available."}`;
  compareActionStatus.className = "lifecycle-action-status";
  compareDetailTitle.focus();
}

async function requestComparePage(reveal, pageToken) {
  if (!compareSession || !compareTableSelection) return;
  compareNextButton.disabled = true;
  compareRevealButton.disabled = true;
  compareActionStatus.textContent = reveal ? "Re-verifying both sources and preparing one explicitly revealed bounded page…" : "Re-verifying both sources and preparing one bounded page…";
  try {
    const page = await invokeHost(reveal ? "reveal_compare_page" : "get_compare_page", {
      request: {
        session_id: compareSession.session_id,
        table_token: compareTableSelection.tableToken,
        page_token: pageToken,
      },
    });
    renderComparePage(page);
  } catch (error) {
    compareActionStatus.textContent = hostError(error);
    compareActionStatus.className = "lifecycle-action-status error";
  } finally {
    compareRevealButton.disabled = false;
    compareNextButton.disabled = !compareNextPageToken;
  }
}

compareChooseButton.addEventListener("click", async () => {
  const selectionId = currentSelectionId();
  if (!selectionId) {
    compareActionStatus.textContent = "Select and inspect a Capsule before choosing its comparison partner.";
    compareActionStatus.className = "lifecycle-action-status error";
    return;
  }
  compareChooseButton.disabled = true;
  compareActionStatus.textContent = "Choose a second local Capsule. It will be pinned and verified without executing application code.";
  compareActionStatus.className = "lifecycle-action-status";
  try {
    const candidate = await invokeHost("choose_compare_capsule", { request: { selection_id: selectionId } });
    if (!candidate) {
      compareActionStatus.textContent = "Comparison selection cancelled. No source was opened.";
      return;
    }
    compareStatus.textContent = `Opaque comparison candidate retained until ${candidate.expires_at}. Verifying the pair…`;
    const session = await invokeHost("start_compare", {
      request: { selection_id: selectionId, candidate_id: candidate.candidate_id },
    });
    renderCompareSession(session);
  } catch (error) {
    resetCompareView("Comparison failed closed. Choose a fresh pair to try again.");
    compareActionStatus.textContent = hostError(error);
    compareActionStatus.className = "lifecycle-action-status error";
  } finally {
    compareChooseButton.disabled = false;
  }
});

compareRevealButton.addEventListener("click", () => requestComparePage(true, null));
compareNextButton.addEventListener("click", () => requestComparePage(comparePageRevealed, compareNextPageToken));
compareCloseButton.addEventListener("click", async () => {
  const sessionId = compareSession?.session_id;
  compareCloseButton.disabled = true;
  try {
    if (sessionId) await invokeHost("close_compare_session", { request: { session_id: sessionId } });
    resetCompareView("Comparison closed. Retained sources and cursors were released.");
    compareActionStatus.textContent = "Comparison session closed.";
    compareActionStatus.className = "lifecycle-action-status";
  } catch (error) {
    compareActionStatus.textContent = hostError(error);
    compareActionStatus.className = "lifecycle-action-status error";
    compareCloseButton.disabled = false;
  }
});

function renderCheckList(target, checks) {
  target.replaceChildren(...(checks || []).map((check) => {
    const item = document.createElement("li");
    item.textContent = compareLabel(check);
    return item;
  }));
}

function reconcileSelectedOrientationToken() {
  return reconcileOrientations.querySelector("input[name='reconcile-orientation']:checked")?.value || null;
}

function selectedReconcileTokens() {
  return [...reconcileSelections.querySelectorAll("input[name='reconcile-selection']:checked")]
    .map((input) => input.value);
}

function selectedReconcileResolutionTokens() {
  return [...reconcileThreeWayWrap.querySelectorAll("input[type='radio']:checked")]
    .map((input) => input.value);
}

function updateReconcilePrepareState() {
  const hasThreeWayAuthority = Boolean(reconcileThreeWay)
    && selectedReconcileResolutionTokens().length === reconcileThreeWay.conflicts.length;
  const hasTwoWayAuthority = !reconcileThreeWay && selectedReconcileTokens().length > 0;
  reconcilePrepareButton.disabled = !reconcileSession || !reconcileDestination
    || (!hasThreeWayAuthority && !hasTwoWayAuthority);
}

function renderReconcileOptions(options) {
  if (options?.profile !== "org.sqlite-capsule.tauri-reconcile-options/1") {
    throw new Error("The host returned an unsupported reconciliation-options profile.");
  }
  reconcileOptions = options;
  const legend = document.createElement("legend");
  legend.textContent = "Choose source and target";
  const choices = options.orientations.map((orientation, index) => {
    const label = document.createElement("label");
    const input = document.createElement("input");
    const copy = document.createElement("span");
    const heading = document.createElement("strong");
    const detail = document.createElement("small");
    input.type = "radio";
    input.name = "reconcile-orientation";
    input.value = orientation.orientation_token;
    input.checked = index === 0;
    heading.append(document.createTextNode("Source "), compareIsolate(orientation.source_label), document.createTextNode(" → target "), compareIsolate(orientation.target_label));
    detail.textContent = `${orientation.result_label}. The original target remains unchanged.`;
    copy.className = "reconcile-choice-copy";
    copy.append(heading, detail);
    label.append(input, copy);
    return label;
  });
  reconcileOrientations.replaceChildren(legend, ...choices);
  reconcileOptionsWrap.hidden = false;
  reconcileStartButton.disabled = options.blockers.length > 0;
  reconcileBadge.textContent = options.blockers.length ? "Needs detail" : `${options.disclosed_change_count.toLocaleString()} disclosed`;
  reconcileBadge.className = `badge ${options.blockers.length ? "warn" : "ok"}`;
  reconcileStatus.textContent = options.blockers.length
    ? `Cannot begin: ${options.blockers.map(compareLabel).join(" · ")}.`
    : `${options.disclosed_change_count.toLocaleString()} disclosed row change${options.disclosed_change_count === 1 ? "" : "s"} available; ${options.sensitive_change_count.toLocaleString()} came from an explicit sensitive reveal.`;
}

reconcileOpenButton.addEventListener("click", async () => {
  if (!compareSession || activeReconcileOperationToken) return;
  reconcileOpenButton.disabled = true;
  reconcileStatus.textContent = "Binding eligible rows from the retained comparison evidence…";
  try {
    const options = await invokeHost("get_reconcile_options", {
      request: { session_token: compareSession.session_id },
    });
    renderReconcileOptions(options);
  } catch (error) {
    reconcileStatus.textContent = hostError(error);
    reconcileStatus.className = "lifecycle-action-status error";
  } finally {
    reconcileOpenButton.disabled = false;
  }
});

reconcileStartButton.addEventListener("click", async () => {
  const orientationToken = reconcileSelectedOrientationToken();
  if (!orientationToken) return;
  reconcileStartButton.disabled = true;
  reconcileStatus.textContent = "Consuming the comparison and fixing one source-to-target direction…";
  try {
    const session = await invokeHost("start_reconcile_review", {
      request: { orientation_token: orientationToken },
    });
    if (session?.profile !== "org.sqlite-capsule.tauri-reconcile-session/1") {
      throw new Error("The host returned an unsupported reconciliation-session profile.");
    }
    reconcileSession = session;
    reconcileThreeWay = null;
    compareSession = null;
    compareCloseButton.disabled = true;
    reconcileOptionsWrap.hidden = true;
    reconcileSessionWrap.hidden = false;
    reconcileIdentity.replaceChildren(
      ...compareDefinition("Source", session.source_label),
      ...compareDefinition("Target", session.target_label),
      ...compareDefinition("Output Capsule ID", session.output_capsule_id),
      ...compareDefinition("Output application digest", session.output_application_digest),
      ...compareDefinition("Output signature inventory", `${Number(session.output_signature_count).toLocaleString()} valid envelope${session.output_signature_count === 1 ? "" : "s"}`),
      ...compareDefinition("Review expires", session.expires_at),
    );
    const legend = document.createElement("legend");
    legend.textContent = "Select disclosed changes";
    const selectionRows = session.selections.map((selection) => {
      const label = document.createElement("label");
      const input = document.createElement("input");
      const copy = document.createElement("span");
      const heading = document.createElement("strong");
      const detail = document.createElement("small");
      input.type = "checkbox";
      input.name = "reconcile-selection";
      input.value = selection.selection_token;
      input.addEventListener("change", updateReconcilePrepareState);
      heading.append(compareIsolate(selection.dataset_label), document.createTextNode(" · "), compareIsolate(selection.table_label));
      detail.textContent = `${compareLabel(selection.action)}${selection.field_count ? ` · ${selection.field_count.toLocaleString()} field${selection.field_count === 1 ? "" : "s"}` : ""} · ${selection.sensitivity}${selection.sensitive_reveal_confirmed && selection.sensitivity === "sensitive" ? " · explicitly revealed" : ""}`;
      copy.className = "reconcile-choice-copy";
      copy.append(heading, detail);
      label.append(input, copy);
      return label;
    });
    reconcileSelections.replaceChildren(legend, ...selectionRows);
    renderCheckList(reconcileSessionChecks, session.checks);
    reconcileConflicts.textContent = "Two-way review has no ancestor-derived conflict resolution. Every operation is guarded by the exact row state disclosed from both retained inputs.";
    reconcileThreeWayWrap.replaceChildren();
    reconcileThreeWayWrap.hidden = true;
    reconcileAncestorButton.disabled = true;
    reconcileBadge.textContent = "Five-minute review";
    reconcileBadge.className = "badge warn";
    reconcileStatus.textContent = `${session.selections.length.toLocaleString()} eligible disclosed change${session.selections.length === 1 ? "" : "s"}. Choose changes and a destination that does not exist.`;
    reconcileOpenButton.disabled = true;
    reconcileIdentity.scrollIntoView({ block: "nearest" });
  } catch (error) {
    reconcileStatus.textContent = hostError(error);
    reconcileStatus.className = "lifecycle-action-status error";
    reconcileStartButton.disabled = false;
  }
});

reconcileDestinationButton.addEventListener("click", async () => {
  if (!reconcileSession) return;
  reconcileDestinationButton.disabled = true;
  reconcileDestinationStatus.textContent = "Choose a path for a new Capsule. Existing files are rejected.";
  reconcileDestinationStatus.className = "boundary-note";
  try {
    const destination = await invokeHost("choose_reconcile_destination", {
      request: { review_token: reconcileSession.review_token },
    });
    if (!destination) {
      reconcileDestinationStatus.textContent = "Destination selection cancelled.";
      return;
    }
    reconcileDestination = destination;
    reconcileDestinationStatus.className = "boundary-note";
    reconcileDestinationStatus.replaceChildren(
      document.createTextNode(`${destination.parent_display} · `),
      compareIsolate(destination.leaf_display),
      document.createTextNode(` · authority expires ${destination.expires_at}`),
    );
    reconcileAncestorButton.disabled = false;
    updateReconcilePrepareState();
  } catch (error) {
    reconcileDestinationStatus.textContent = hostError(error);
    reconcileDestinationStatus.className = "boundary-note error";
  } finally {
    reconcileDestinationButton.disabled = false;
  }
});

reconcileAncestorButton.addEventListener("click", async () => {
  if (!reconcileSession || !reconcileDestination || reconcileThreeWay) return;
  reconcileAncestorButton.disabled = true;
  reconcilePrepareButton.disabled = true;
  reconcileDestinationButton.disabled = true;
  reconcileStatus.textContent = "Choose a verified common ancestor. The host keeps its path and exact snapshot private.";
  try {
    const threeWay = await invokeHost("choose_reconcile_ancestor", {
      request: {
        review_token: reconcileSession.review_token,
        destination_token: reconcileDestination.destination_token,
      },
    });
    if (!threeWay) {
      reconcileStatus.textContent = "Ancestor selection cancelled. Two-way review remains available.";
      reconcileAncestorButton.disabled = false;
      reconcileDestinationButton.disabled = false;
      updateReconcilePrepareState();
      return;
    }
    if (threeWay.profile !== "org.sqlite-capsule.tauri-reconcile-three-way/1") {
      throw new Error("The host returned an unsupported three-way reconciliation profile.");
    }
    reconcileThreeWay = threeWay;
    reconcileSelections.querySelectorAll("input").forEach((input) => { input.disabled = true; });
    reconcileConflicts.textContent = `${threeWay.clean_change_count.toLocaleString()} clean three-way change${threeWay.clean_change_count === 1 ? "" : "s"}; ${threeWay.conflicts.length.toLocaleString()} conflict${threeWay.conflicts.length === 1 ? "" : "s"} must be resolved before the short authority expires ${threeWay.expires_at}.`;
    const ancestor = document.createElement("article");
    const ancestorHeading = document.createElement("strong");
    const ancestorDetail = document.createElement("span");
    ancestorHeading.textContent = "Verified ancestor";
    ancestorDetail.append(
      compareIsolate(threeWay.ancestor.capsule_id),
      document.createTextNode(` · revision ${compareDisplayText(threeWay.ancestor.revision_id)} · schema v${Number(threeWay.ancestor.data_schema_version).toLocaleString()}`),
    );
    ancestor.append(ancestorHeading, ancestorDetail);
    const conflicts = threeWay.conflicts.map((conflict, index) => {
      const fieldset = document.createElement("fieldset");
      const legend = document.createElement("legend");
      legend.append(
        document.createTextNode(`${index + 1}. `),
        compareIsolate(conflict.dataset_label),
        document.createTextNode(" · "),
        compareIsolate(conflict.table_label),
        document.createTextNode(` · ${compareLabel(conflict.kind)}${conflict.deleted_side ? ` · ${compareLabel(conflict.deleted_side)} deleted` : ""}`),
      );
      fieldset.append(legend);
      conflict.choices.forEach((choice) => {
        const label = document.createElement("label");
        const input = document.createElement("input");
        const copy = document.createElement("span");
        input.type = "radio";
        input.name = `reconcile-conflict-${conflict.conflict_token}`;
        input.value = choice.resolution_token;
        input.addEventListener("change", updateReconcilePrepareState);
        copy.textContent = compareLabel(choice.choice);
        label.append(input, copy);
        fieldset.append(label);
      });
      return fieldset;
    });
    reconcileThreeWayWrap.replaceChildren(ancestor, ...conflicts);
    reconcileThreeWayWrap.hidden = false;
    reconcileBadge.textContent = threeWay.conflicts.length ? "Resolve conflicts" : "Three-way clean";
    reconcileBadge.className = `badge ${threeWay.conflicts.length ? "warn" : "ok"}`;
    reconcileStatus.textContent = threeWay.conflicts.length
      ? "Resolve every conflict explicitly; immutable-field conflicts permit only keeping the target."
      : "The verified ancestor classified every change as clean. Prepare the exact target-derived review now.";
    updateReconcilePrepareState();
  } catch (error) {
    reconcileStatus.textContent = hostError(error);
    reconcileStatus.className = "lifecycle-action-status error";
    reconcileBadge.textContent = "Ancestor rejected";
    reconcileBadge.className = "badge fail";
  }
});

reconcilePrepareButton.addEventListener("click", async () => {
  if (!reconcileSession || !reconcileDestination) return;
  const selectionTokens = selectedReconcileTokens();
  const resolutionTokens = selectedReconcileResolutionTokens();
  if (!reconcileThreeWay && !selectionTokens.length) return;
  if (reconcileThreeWay && resolutionTokens.length !== reconcileThreeWay.conflicts.length) return;
  reconcilePrepareButton.disabled = true;
  reconcileDestinationButton.disabled = true;
  reconcileStatus.textContent = "Recomputing comparison, resolving opaque selections and dry-running all signed datasets…";
  try {
    const review = await invokeHost("prepare_reconcile", {
      request: {
        review_token: reconcileSession.review_token,
        destination_token: reconcileDestination.destination_token,
        selection_tokens: reconcileThreeWay ? [] : selectionTokens,
        ancestor_token: reconcileThreeWay?.ancestor_token ?? null,
        resolution_tokens: reconcileThreeWay ? resolutionTokens : [],
      },
    });
    if (review?.profile !== "org.sqlite-capsule.tauri-reconcile-review/1") {
      throw new Error("The host returned an unsupported reconciliation-review profile.");
    }
    preparedReconcile = review;
    reconcileSessionWrap.hidden = true;
    reconcileReviewWrap.hidden = false;
    reconcileReviewIdentity.replaceChildren(
      ...compareDefinition("Source", `${review.source.capsule_id} · revision ${review.source.revision_id}`),
      ...compareDefinition("Target", `${review.target.capsule_id} · revision ${review.target.revision_id}`),
      ...compareDefinition("Output", `${review.output.capsule_id} · new revision ${review.output.revision_id}`),
      ...compareDefinition("Application", review.output.application_digest),
      ...compareDefinition("Signature inventory", `${Number(review.output.signature_count).toLocaleString()} valid envelope${review.output.signature_count === 1 ? "" : "s"}`),
      ...compareDefinition("Data schema", `${review.output.data_schema_id} v${review.output.data_schema_version}`),
      ...compareDefinition("Review digest", review.review_digest),
      ...compareDefinition("Value-free payload digest", review.payload_digest),
      ...compareDefinition("Lineage parents", review.lineage_parent_count),
      ...compareDefinition("New destination", review.destination.leaf_display),
      ...compareDefinition("Execution expires", review.expires_at),
    );
    reconcileOperationList.replaceChildren(...review.operations.map((operation) => {
      const article = document.createElement("article");
      const heading = document.createElement("strong");
      const detail = document.createElement("span");
      heading.append(document.createTextNode(`${operation.sequence.toLocaleString()}. `), compareIsolate(operation.dataset_label), document.createTextNode(" · "), compareIsolate(operation.table_label));
      detail.textContent = `${compareLabel(operation.action)}${operation.field_count ? ` · ${operation.field_count.toLocaleString()} field${operation.field_count === 1 ? "" : "s"}` : ""}${operation.sensitive_confirmed ? " · sensitive reveal confirmed" : ""}`;
      article.append(heading, detail);
      return article;
    }));
    renderCheckList(reconcileReviewChecks, review.checks);
    reconcileConfirmation.checked = false;
    reconcileExecuteButton.disabled = true;
    reconcileBadge.textContent = "Exact review ready";
    reconcileBadge.className = "badge ok";
    reconcileStatus.textContent = `${review.operation_count.toLocaleString()} exact operation${review.operation_count === 1 ? "" : "s"} prepared. Confirm before the separate 30-second authority expires.`;
    reconcileReviewTitle.focus();
  } catch (error) {
    reconcileStatus.textContent = hostError(error);
    reconcileStatus.className = "lifecycle-action-status error";
    reconcileBadge.textContent = "Review rejected";
    reconcileBadge.className = "badge fail";
  }
});

reconcileConfirmation.addEventListener("change", () => {
  reconcileExecuteButton.disabled = !preparedReconcile || !reconcileConfirmation.checked || Boolean(activeReconcileOperationToken);
});

function finishReconcileOperation(operationToken) {
  return oncePerReconcileOperation(operationToken, async () => {
    try {
      const status = await invokeHost("get_reconcile_operation", {
        request: { operation_token: operationToken },
      });
      if (status.operation_token !== operationToken) return;
      reconcileResult.hidden = false;
      reconcileResultOutput.textContent = status.phase === "succeeded"
        ? `Verified new Capsule\n${compareDisplayText(status.output_leaf)}\n${Number(status.output_bytes).toLocaleString()} bytes\nBoth retained inputs remained unchanged.`
        : `${compareLabel(status.phase)}\n${hostError(status.error || "Operation did not complete.")}`;
      reconcileBadge.textContent = compareLabel(status.phase);
      reconcileBadge.className = `badge ${status.phase === "succeeded" ? "ok" : "fail"}`;
      reconcileStatus.textContent = status.phase === "succeeded"
        ? "The new Capsule was reopened, exhaustively validated and rebound before publication completed."
        : `Reconciliation ${compareLabel(status.phase)}. No unverified output is accepted.`;
      reconcileStatus.className = `lifecycle-action-status${status.phase === "succeeded" ? "" : " error"}`;
      await invokeHost("acknowledge_reconcile_result", {
        request: { operation_token: operationToken },
      });
      if (activeReconcileOperationToken === operationToken) {
        activeReconcileOperationToken = null;
      }
      reconcileCancelButton.disabled = true;
      reconcileExecuteButton.disabled = true;
    } catch (error) {
      reconcileStatus.textContent = `Could not read the terminal reconciliation result: ${hostError(error)}`;
      reconcileStatus.className = "lifecycle-action-status error";
    }
  });
}

function oncePerReconcileOperation(operationToken, finalizer) {
  const existing = reconcileFinalizations.get(operationToken);
  if (existing) return existing;
  const finalization = Promise.resolve().then(finalizer);
  if (reconcileFinalizations.size >= 64) {
    reconcileFinalizations.delete(reconcileFinalizations.keys().next().value);
  }
  reconcileFinalizations.set(operationToken, finalization);
  return finalization;
}

async function reconcileStatusAfterStart(operationToken) {
  const inFlightFinalization = reconcileFinalizations.get(operationToken);
  if (inFlightFinalization) {
    await inFlightFinalization;
    return;
  }
  let status;
  try {
    status = await invokeHost("get_reconcile_operation", {
      request: { operation_token: operationToken },
    });
  } catch (error) {
    const terminalEventFinalization = reconcileFinalizations.get(operationToken);
    if (terminalEventFinalization) {
      await terminalEventFinalization;
      return;
    }
    throw error;
  }
  if (status.operation_token !== operationToken) return;
  if (["succeeded", "failed", "cancelled"].includes(status.phase)) {
    await finishReconcileOperation(operationToken);
    return;
  }
  if (activeReconcileOperationToken === operationToken) {
    globalThis.setTimeout(() => {
      if (activeReconcileOperationToken === operationToken) {
        void reconcileStatusAfterStart(operationToken).catch((error) => {
          reconcileStatus.textContent = `Could not poll reconciliation status: ${hostError(error)}`;
          reconcileStatus.className = "lifecycle-action-status error";
        });
      }
    }, 250);
  }
}

reconcileExecuteButton.addEventListener("click", async () => {
  if (!preparedReconcile || !reconcileConfirmation.checked) return;
  reconcileExecuteButton.disabled = true;
  reconcileStatus.textContent = "Consuming the one-use confirmation authority…";
  try {
    const status = await invokeHost("execute_reconcile", {
      request: { confirmation_nonce: preparedReconcile.confirmation_nonce },
    });
    activeReconcileOperationToken = status.operation_token;
    reconcileCancelButton.disabled = !status.cancellable;
    reconcileBadge.textContent = compareLabel(status.phase);
    reconcileBadge.className = "badge warn";
    await reconcileStatusAfterStart(status.operation_token);
  } catch (error) {
    reconcileStatus.textContent = hostError(error);
    reconcileStatus.className = "lifecycle-action-status error";
    reconcileBadge.textContent = "Execution rejected";
    reconcileBadge.className = "badge fail";
  }
});

reconcileCancelButton.addEventListener("click", async () => {
  if (!activeReconcileOperationToken) return;
  reconcileCancelButton.disabled = true;
  try {
    await invokeHost("cancel_reconcile_operation", {
      request: { operation_token: activeReconcileOperationToken },
    });
    reconcileStatus.textContent = "Cancellation requested. Publication becomes non-cancellable before the no-replace rename.";
  } catch (error) {
    reconcileStatus.textContent = hostError(error);
    reconcileStatus.className = "lifecycle-action-status error";
  }
});

function selectedCopyMode() {
  return copyModeGrid.querySelector("input[name='copy-mode']:checked")?.value || "exact-duplicate";
}

function currentSelectionId() {
  return currentReport?.selection_id ?? currentReport?.capsule?.overview?.selection_id ?? null;
}

function resetCopyReview(message = "Choose a create-new destination to prepare a review.") {
  if (activeCopyOperationId) {
    copyActionStatus.textContent = "A copy operation is already active. Cancel it or wait for its verified terminal result.";
    copyActionStatus.className = "lifecycle-action-status error";
    return false;
  }
  copyDestination = null;
  copyProfilePreview = null;
  preparedCopy = null;
  activeCopyOperationId = null;
  copyReview.hidden = true;
  copyConsent.hidden = true;
  copyConfirmation.checked = false;
  copyExecuteButton.disabled = true;
  copyPrepareButton.disabled = true;
  copyResultWrap.hidden = true;
  copyDatasetReview.replaceChildren();
  copyProfileDatasets.replaceChildren();
  copyProfileReview.hidden = true;
  copyDestinationStatus.textContent = message;
  copyBadge.textContent = "No review";
  copyBadge.className = "badge";
  return true;
}

function setCopyControlsLocked(locked) {
  copyModeGrid.querySelectorAll("input[name='copy-mode']").forEach((input) => { input.disabled = locked; });
  copyDestinationButton.disabled = locked;
  copyPrepareButton.disabled = locked || !copyDestination;
  copyClearButton.textContent = locked ? "Cancel operation" : "Clear review";
}

function renderCopyProfilePreview(preview) {
  copyProfilePreview = preview;
  copyStatus.dataset.mode = preview.mode;
  copyBadge.textContent = preview.blockers.length ? "Blocked" : "Profile reviewed";
  copyBadge.className = `badge ${preview.blockers.length ? "fail" : "ok"}`;
  copyProfileReview.hidden = preview.datasets.length === 0;
  copyProfileDatasets.replaceChildren(...preview.datasets.map((dataset) => {
    const article = document.createElement("article");
    const title = document.createElement("strong");
    const detail = document.createElement("span");
    title.textContent = dataset.dataset_id;
    const dependencyDetail = dataset.dependencies.length
      ? ` · depends on ${dataset.dependencies.join(", ")}`
      : "";
    const selectedDetail = dataset.auto_selected_by_dependency
      ? " · included because another selected dataset depends on it"
      : "";
    detail.textContent = `${dataset.sensitivity} · signed ${dataset.signed_fork_policy}${dependencyDetail}${selectedDetail}`;
    article.append(title, detail);
    if (dataset.choice_id) {
      const label = document.createElement("label");
      label.textContent = "Dataset action ";
      const select = document.createElement("select");
      select.dataset.choiceId = dataset.choice_id;
      if (dataset.allow_include) select.append(new Option("Include", "include"));
      if (dataset.allow_omit) select.append(new Option("Omit", "omit"));
      select.value = dataset.default_disposition || (dataset.allow_omit ? "omit" : "include");
      select.addEventListener("change", () => {
        const checkbox = copyProfileDatasets.querySelector(`[data-sensitive-choice-id='${dataset.choice_id}']`);
        if (checkbox) {
          checkbox.disabled = select.value !== "include";
          if (checkbox.disabled) checkbox.checked = false;
        }
      });
      label.append(select);
      article.append(label);
      if (dataset.sensitive_confirmation_required) {
        const confirm = document.createElement("label");
        const checkbox = document.createElement("input");
        checkbox.type = "checkbox";
        checkbox.dataset.sensitiveChoiceId = dataset.choice_id;
        checkbox.disabled = select.value !== "include";
        confirm.append(checkbox, document.createTextNode(" Explicitly include sensitive data"));
        article.append(confirm);
      }
    } else {
      const fixed = document.createElement("span");
      fixed.textContent = `Host action: ${dataset.fixed_action}`;
      article.append(fixed);
    }
    return article;
  }));
  copyStatus.textContent = preview.blockers.length
    ? `This profile is blocked: ${preview.blockers.join(" · ")}`
    : "Profile inspected. Choose a create-new destination when the dataset decisions are correct.";
  copyDestinationButton.disabled = preview.blockers.length > 0;
}

async function loadCopyProfilePreview() {
  const selectionId = currentSelectionId();
  if (!selectionId) throw new Error("Select and inspect a Capsule first.");
  const preview = await invokeHost("preview_copy_profile", {
    request: { selection_id: selectionId, mode: selectedCopyMode() },
  });
  renderCopyProfilePreview(preview);
  return preview;
}

function submittedCopyChoices() {
  if (!copyProfilePreview) return [];
  return copyProfilePreview.datasets.flatMap((dataset) => {
    if (!dataset.choice_id) return [];
    const disposition = copyProfileDatasets.querySelector(`[data-choice-id='${dataset.choice_id}']`)?.value;
    const sensitiveConfirmed = Boolean(copyProfileDatasets.querySelector(`[data-sensitive-choice-id='${dataset.choice_id}']`)?.checked);
    return [{
      choice_id: dataset.choice_id,
      disposition,
      sensitive_confirmed: sensitiveConfirmed,
    }];
  });
}

function renderPreparedCopy(review) {
  preparedCopy = review;
  copyReview.hidden = false;
  copyConsent.hidden = false;
  copyConfirmation.checked = false;
  copyExecuteButton.disabled = true;
  copyBadge.textContent = "Review ready";
  copyBadge.className = "badge warn";
  const source = review.preview || {};
  setRows(copyReviewDetails, [
    ["Operation", review.mode],
    ["Plan", review.plan_id],
    ["Plan digest", review.plan_digest],
    ["Source format", source.format_version || source.source_format_version],
    ["Source SHA-256", source.source_sha256 || "Bound in the retained plan"],
    ["Signature state", source.signature_state || `${source.signature_count || 0} valid signature(s)`],
    ["Publisher trust", currentReport?.capsule?.overview?.application?.publisher?.host_trust || "separate host policy"],
    ["Capsule identity", source.capsule_identity],
    ["Revision identity", source.revision_identity],
    ["Application digest", source.application_digest || source.expected_output_sha256 || "Preserved"],
    ["Destination", `${review.output.parent_display} · ${review.output.leaf_display}`],
    ["Overwrite", source.overwrite_allowed ? "Allowed" : "Never"],
    ["Expires", review.expires_at],
    ["Checks", review.checks.join(" · ")],
  ]);
  const datasets = source.datasets || [];
  copyDatasetReview.replaceChildren(...datasets.map((dataset) => {
    const article = document.createElement("article");
    const title = document.createElement("strong");
    const detail = document.createElement("span");
    title.textContent = dataset.dataset_id;
    detail.textContent = `${dataset.action} · ${dataset.sensitivity} · ${dataset.source_row_count.toLocaleString()} source rows`;
    article.append(title, detail);
    return article;
  }));
}

copyModeGrid.addEventListener("change", async () => {
  if (activeCopyOperationId) return;
  if (copyDestination) {
    try {
      await invokeHost("cancel_copy_destination", { request: { destination_id: copyDestination.destination_id } });
    } catch (_) { /* stale destination authority is already unusable */ }
  }
  resetCopyReview("The copy profile changed. Choose a new create-new destination.");
  try {
    await loadCopyProfilePreview();
  } catch (error) {
    copyActionStatus.textContent = hostError(error);
    copyActionStatus.className = "lifecycle-action-status error";
  }
});

copyDestinationButton.addEventListener("click", async () => {
  const selectionId = currentSelectionId();
  if (!selectionId) {
    copyActionStatus.textContent = "Select and inspect a Capsule before choosing a destination.";
    copyActionStatus.className = "lifecycle-action-status error";
    return;
  }
  copyDestinationButton.disabled = true;
  copyActionStatus.textContent = "Choose a new local Capsule filename. Existing files and destination sidecars are refused.";
  copyActionStatus.className = "lifecycle-action-status";
  try {
    if (!copyProfilePreview || copyProfilePreview.mode !== selectedCopyMode()) {
      await loadCopyProfilePreview();
    }
    if (copyProfilePreview.blockers.length) throw new Error("The selected signed policy blocks this profile.");
    const destination = await invokeHost("choose_copy_destination", {
      request: { selection_id: selectionId, mode: selectedCopyMode() },
    });
    if (!destination) {
      copyActionStatus.textContent = "Destination selection cancelled. Nothing was created.";
      return;
    }
    copyDestination = destination;
    preparedCopy = null;
    copyPrepareButton.disabled = false;
    copyDestinationStatus.textContent = `${destination.parent_display} · ${destination.leaf_display} · expires ${destination.expires_at}`;
    copyActionStatus.textContent = "Opaque create-new destination retained by Rust. Prepare a fresh source-bound review.";
  } catch (error) {
    resetCopyReview("Destination selection failed closed. Choose another create-new destination.");
    copyActionStatus.textContent = hostError(error);
    copyActionStatus.className = "lifecycle-action-status error";
  } finally {
    copyDestinationButton.disabled = false;
  }
});

copyPrepareButton.addEventListener("click", async () => {
  const selectionId = currentSelectionId();
  if (!selectionId || !copyDestination) return;
  copyPrepareButton.disabled = true;
  copyActionStatus.textContent = "Re-verifying the selected source and binding the held destination…";
  copyActionStatus.className = "lifecycle-action-status";
  try {
    const review = await invokeHost("prepare_copy", {
      request: {
        selection_id: selectionId,
        destination_id: copyDestination.destination_id,
        mode: selectedCopyMode(),
        choices: submittedCopyChoices(),
      },
    });
    renderPreparedCopy(review);
    copyActionStatus.textContent = "Review prepared. The confirmation nonce is one-use and expires with this native authority.";
  } catch (error) {
    resetCopyReview("The prepared authority was refused. Choose a fresh destination.");
    copyActionStatus.textContent = hostError(error);
    copyActionStatus.className = "lifecycle-action-status error";
  }
});

copyConfirmation.addEventListener("change", () => {
  copyExecuteButton.disabled = !copyConfirmation.checked || !preparedCopy;
});

copyExecuteButton.addEventListener("click", async () => {
  if (!preparedCopy || !copyConfirmation.checked) return;
  copyExecuteButton.disabled = true;
  copyActionStatus.textContent = "Starting the one-use native copy operation…";
  copyActionStatus.className = "lifecycle-action-status";
  try {
    const status = await invokeHost("execute_copy", {
      request: {
        plan_id: preparedCopy.plan_id,
        confirmation_nonce: preparedCopy.confirmation_nonce,
      },
    });
    activeCopyOperationId = status.operation_id;
    preparedCopy = null;
    setCopyControlsLocked(true);
    copyBadge.textContent = "Running";
    copyBadge.className = "badge warn";
    copyActionStatus.textContent = "Source re-verification and private create-new transformation are running.";
    const current = await invokeHost("get_copy_operation", {
      request: { operation_id: activeCopyOperationId },
    });
    if (["succeeded", "failed", "cancelled"].includes(current.phase)) {
      await finishCopyOperation(activeCopyOperationId);
    }
  } catch (error) {
    copyActionStatus.textContent = hostError(error);
    copyActionStatus.className = "lifecycle-action-status error";
  }
});

copyClearButton.addEventListener("click", async () => {
  if (activeCopyOperationId) {
    copyClearButton.disabled = true;
    try {
      await invokeHost("cancel_copy_operation", { request: { operation_id: activeCopyOperationId } });
      copyActionStatus.textContent = "Cancellation requested. Publication may finish if the no-replace boundary has already started.";
      copyActionStatus.className = "lifecycle-action-status";
    } catch (error) {
      copyActionStatus.textContent = hostError(error);
      copyActionStatus.className = "lifecycle-action-status error";
    } finally {
      copyClearButton.disabled = false;
    }
    return;
  }
  if (copyDestination && !preparedCopy) {
    try {
      await invokeHost("cancel_copy_destination", { request: { destination_id: copyDestination.destination_id } });
    } catch (_) { /* already consumed or stale */ }
  }
  resetCopyReview("Review cleared. No destination or plan authority is retained by this page.");
  copyActionStatus.textContent = "Copy review cleared.";
  copyActionStatus.className = "lifecycle-action-status";
});

async function finishCopyOperation(operationId) {
  try {
    const status = await invokeHost("get_copy_operation", { request: { operation_id: operationId } });
    if (activeCopyOperationId !== operationId) return;
    copyResultWrap.hidden = false;
    const succeeded = status.phase === "succeeded";
    copyResult.textContent = succeeded
      ? `${status.mode}\n${status.output_leaf}\n${Number(status.output_bytes || 0).toLocaleString()} bytes\nVerified, create-new, source unchanged`
      : `${status.mode}\n${status.phase}\n${hostError(status.error || "The operation failed closed.")}`;
    copyBadge.textContent = succeeded ? "Verified" : status.phase === "cancelled" ? "Cancelled" : "Failed closed";
    copyBadge.className = `badge ${succeeded ? "ok" : status.phase === "cancelled" ? "warn" : "fail"}`;
    copyActionStatus.textContent = succeeded
      ? "The new Capsule was exhaustively reopened and verified. The source remained unchanged."
      : hostError(status.error || "The operation did not report success.");
    copyActionStatus.className = succeeded ? "lifecycle-action-status" : "lifecycle-action-status error";
    await invokeHost("acknowledge_copy_result", { request: { operation_id: operationId } });
    activeCopyOperationId = null;
    copyDestination = null;
    preparedCopy = null;
    copyReview.hidden = true;
    copyConsent.hidden = true;
    copyPrepareButton.disabled = true;
    copyExecuteButton.disabled = true;
    setCopyControlsLocked(false);
  } catch (error) {
    copyActionStatus.textContent = hostError(error);
    copyActionStatus.className = "lifecycle-action-status error";
  }
}

function resetUpgradeView(message = "Choose a clean, signed release of the same application and schema.") {
  if (activeUpgradeOperationToken) return false;
  upgradeCandidate = null;
  upgradeDestination = null;
  preparedUpgrade = null;
  upgradeCandidateDetails.hidden = true;
  upgradeCandidateDetails.replaceChildren();
  upgradeReview.hidden = true;
  upgradeReviewDetails.replaceChildren();
  upgradeCapabilities.replaceChildren();
  upgradeDatasets.replaceChildren();
  upgradeChecks.replaceChildren();
  upgradePublisherConfirmation.checked = false;
  upgradeCapabilityConfirmation.checked = false;
  upgradeCapabilityConfirmationWrap.hidden = true;
  upgradeDestinationButton.disabled = true;
  upgradePrepareButton.disabled = true;
  upgradeExecuteButton.disabled = true;
  upgradeCancelButton.disabled = true;
  upgradeResult.hidden = true;
  upgradeReleaseStatus.textContent = message;
  upgradeDestinationStatus.textContent = "Choose a release first. Existing files are never overwritten.";
  upgradeStatus.textContent = "Both retained inputs remain pinned read-only. The upgraded Capsule is always a verified new file.";
  upgradeStatus.className = "";
  upgradeActionStatus.textContent = "";
  upgradeActionStatus.className = "lifecycle-action-status";
  upgradeBadge.textContent = "No release";
  upgradeBadge.className = "badge";
  return true;
}

function upgradeArticle(titleText, detailText) {
  const article = document.createElement("article");
  const title = document.createElement("strong");
  const detail = document.createElement("span");
  title.textContent = titleText;
  detail.textContent = detailText;
  article.append(title, detail);
  return article;
}

function renderUpgradeCandidate(candidate) {
  upgradeCandidate = candidate;
  upgradeDestination = null;
  preparedUpgrade = null;
  upgradeReview.hidden = true;
  upgradeResult.hidden = true;
  upgradeCandidateDetails.hidden = false;
  setRows(upgradeCandidateDetails, [
    ["Application", candidate.app_id],
    ["Version change", `${candidate.source_version} → ${candidate.target_version}`],
    ["Data schema", `${candidate.data_schema_id} v${candidate.data_schema_version}`],
    ["Accepted publisher key", candidate.publisher_key_id],
    ["Release file", candidate.release_file_display],
    ["Selection expires", candidate.expires_at],
  ]);
  upgradeReleaseStatus.textContent = `${candidate.release_file_display} · ${candidate.source_version} → ${candidate.target_version}`;
  upgradeDestinationStatus.textContent = "Release screened. Choose a filename that does not exist.";
  upgradeDestinationButton.disabled = false;
  upgradePrepareButton.disabled = true;
  upgradeBadge.textContent = "Release screened";
  upgradeBadge.className = "badge warn";
  upgradeStatus.textContent = "The target signature inventory, retained publisher key and clean-template proof passed initial screening. Same-application, schema and newer-version admission runs when preparing the exact review.";
}

function renderPreparedUpgrade(prepared) {
  preparedUpgrade = prepared;
  const review = prepared.review;
  upgradeReview.hidden = false;
  upgradePublisherConfirmation.checked = false;
  upgradeCapabilityConfirmation.checked = false;
  upgradeCapabilityConfirmationWrap.hidden = !review.capability_delta.requires_review;
  upgradeExecuteButton.disabled = true;
  upgradeBadge.textContent = "Review ready";
  upgradeBadge.className = "badge warn";
  setRows(upgradeReviewDetails, [
    ["Operation", "Same-schema application upgrade"],
    ["Source release", `${review.source.app_version} · ${shortDigest(review.source.application_digest)}`],
    ["Source file SHA-256", review.source.file_sha256],
    ["Target release", `${review.target_release.app_version} · ${shortDigest(review.target_release.application_digest)}`],
    ["Target file SHA-256", review.target_release.file_sha256],
    ["Accepted publisher key", review.publisher_continuity.accepted_key_id],
    ["Publisher continuity", review.publisher_continuity.state],
    ["Data schema", `${review.output.data_schema_id} v${review.output.data_schema_version}`],
    ["New Capsule identity", review.output.capsule_id],
    ["New revision", review.output.revision_id],
    ["Output application", `${review.output.app_id} ${review.output.app_version}`],
    ["Target clean-state proof", review.target_template_state_sha256],
    ["Upgrade review digest", review.review_digest],
    ["Lifecycle plan digest", prepared.plan_digest],
    ["Lineage", `${review.lineage.working_relation} + ${review.lineage.release_relation}`],
    ["Destination", `${prepared.output.parent_display} · ${prepared.output.leaf_display}`],
    ["Limits", `${review.limits.max_rows_written.toLocaleString()} rows · ${review.limits.max_output_bytes.toLocaleString()} bytes · ${review.limits.deadline_ms.toLocaleString()} ms`],
    ["Expires", prepared.expires_at],
  ]);
  const delta = review.capability_delta;
  const capabilityGroups = [
    ["Added", delta.added],
    ["Changed", delta.changed],
    ["Removed", delta.removed],
  ];
  upgradeCapabilities.replaceChildren(...capabilityGroups.map(([label, values]) => upgradeArticle(
    label,
    values.length ? values.join(" · ") : "None",
  )));
  upgradeDatasets.replaceChildren(...review.dataset_actions.map((dataset) => upgradeArticle(
    dataset.dataset_id,
    `${compareLabel(dataset.action)} · signed ${dataset.policy} policy · source ${dataset.source.row_count.toLocaleString()} rows → expected ${dataset.expected.row_count.toLocaleString()} rows · ${shortDigest(dataset.expected.state_sha256)}`,
  )));
  upgradeChecks.replaceChildren(...prepared.checks.map((check) => {
    const item = document.createElement("li");
    item.textContent = check;
    return item;
  }));
  upgradeActionStatus.textContent = review.capability_delta.requires_review
    ? "Added or changed capabilities require a separate explicit confirmation before execution."
    : "No capability increase was detected. Confirm the exact publisher key and bound inputs before execution.";
  upgradeActionStatus.className = "lifecycle-action-status";
  upgradeReviewTitle.focus();
}

function refreshUpgradeExecuteButton() {
  const capabilityAccepted = upgradeCapabilityConfirmationWrap.hidden || upgradeCapabilityConfirmation.checked;
  upgradeExecuteButton.disabled = !preparedUpgrade || !upgradePublisherConfirmation.checked || !capabilityAccepted;
}

upgradeReleaseButton.addEventListener("click", async () => {
  const selectionId = currentSelectionId();
  if (!selectionId || activeUpgradeOperationToken) {
    upgradeActionStatus.textContent = activeUpgradeOperationToken
      ? "An upgrade is already running."
      : "Select and inspect a working Capsule before choosing an application release.";
    upgradeActionStatus.className = "lifecycle-action-status error";
    return;
  }
  resetUpgradeView("Choose one clean, signed target release. Application code will not run.");
  upgradeReleaseButton.disabled = true;
  upgradeActionStatus.textContent = "Inspecting the target release signature inventory and clean-template proof…";
  try {
    const candidate = await invokeHost("choose_upgrade_release", {
      request: { selection_id: selectionId },
    });
    if (!candidate) {
      upgradeActionStatus.textContent = "Release selection cancelled. No authority was retained by this page.";
      return;
    }
    if (candidate.profile !== "org.sqlite-capsule.tauri-upgrade-candidate/1") {
      throw new Error("The host returned an unsupported upgrade-candidate profile.");
    }
    renderUpgradeCandidate(candidate);
    upgradeActionStatus.textContent = "Signed release screened. The host retains its exact read-only path behind an opaque token until full admission at Prepare.";
  } catch (error) {
    resetUpgradeView("Target release screening failed closed. Choose a different signed release.");
    upgradeActionStatus.textContent = hostError(error);
    upgradeActionStatus.className = "lifecycle-action-status error";
    upgradeBadge.textContent = "Release rejected";
    upgradeBadge.className = "badge fail";
  } finally {
    upgradeReleaseButton.disabled = false;
  }
});

upgradeDestinationButton.addEventListener("click", async () => {
  if (!upgradeCandidate || activeUpgradeOperationToken) return;
  upgradeDestinationButton.disabled = true;
  upgradeActionStatus.textContent = "Choose a new local Capsule filename. Existing outputs and either input path are refused.";
  upgradeActionStatus.className = "lifecycle-action-status";
  try {
    const destination = await invokeHost("choose_upgrade_destination", {
      request: { candidate_token: upgradeCandidate.candidate_token },
    });
    if (!destination) {
      upgradeActionStatus.textContent = "Destination selection cancelled. Nothing was created.";
      return;
    }
    upgradeDestination = destination;
    preparedUpgrade = null;
    upgradeReview.hidden = true;
    upgradeDestinationStatus.textContent = `${destination.parent_display} · ${destination.leaf_display} · expires ${destination.expires_at}`;
    upgradePrepareButton.disabled = false;
    upgradeActionStatus.textContent = "Opaque create-new destination retained. Prepare an exact two-input review.";
  } catch (error) {
    upgradeDestination = null;
    upgradePrepareButton.disabled = true;
    upgradeDestinationStatus.textContent = "Destination selection failed closed. Choose another new filename.";
    upgradeActionStatus.textContent = hostError(error);
    upgradeActionStatus.className = "lifecycle-action-status error";
  } finally {
    upgradeDestinationButton.disabled = !upgradeCandidate;
  }
});

upgradePrepareButton.addEventListener("click", async () => {
  const selectionId = currentSelectionId();
  if (!selectionId || !upgradeCandidate || !upgradeDestination) return;
  upgradePrepareButton.disabled = true;
  upgradeActionStatus.textContent = "Rebinding both exact files and deriving the exhaustive same-schema upgrade plan…";
  upgradeActionStatus.className = "lifecycle-action-status";
  try {
    const prepared = await invokeHost("prepare_upgrade", {
      request: {
        selection_id: selectionId,
        candidate_token: upgradeCandidate.candidate_token,
        destination_token: upgradeDestination.destination_token,
      },
    });
    if (prepared.profile !== "org.sqlite-capsule.tauri-upgrade-review/1") {
      throw new Error("The host returned an unsupported upgrade-review profile.");
    }
    renderPreparedUpgrade(prepared);
  } catch (error) {
    upgradeCandidate = null;
    upgradeDestination = null;
    preparedUpgrade = null;
    upgradeDestinationButton.disabled = true;
    upgradeReview.hidden = true;
    upgradeActionStatus.textContent = hostError(error);
    upgradeActionStatus.className = "lifecycle-action-status error";
    upgradeBadge.textContent = "Review rejected";
    upgradeBadge.className = "badge fail";
  }
});

[upgradePublisherConfirmation, upgradeCapabilityConfirmation].forEach((input) => {
  input.addEventListener("change", refreshUpgradeExecuteButton);
});

async function finishUpgradeOperation(operationToken) {
  return oncePerUpgradeOperation(operationToken, async () => {
    try {
      const status = await invokeHost("get_upgrade_operation", {
        request: { operation_token: operationToken },
      });
      if (status.operation_token !== operationToken) return;
      const succeeded = status.phase === "succeeded";
      upgradeResult.hidden = false;
      upgradeResultOutput.textContent = succeeded
        ? `${status.output_leaf}\n${Number(status.output_bytes || 0).toLocaleString()} bytes\nStrictly newer same-schema application release\nBoth retained inputs unchanged`
        : `${compareLabel(status.phase)}\n${hostError(status.error || "The operation failed closed.")}`;
      upgradeBadge.textContent = succeeded ? "Verified" : status.phase === "cancelled" ? "Cancelled" : "Failed closed";
      upgradeBadge.className = `badge ${succeeded ? "ok" : status.phase === "cancelled" ? "warn" : "fail"}`;
      upgradeStatus.textContent = succeeded
        ? "The new Capsule was reopened and exhaustively verified with target application bytes and preserved instance data."
        : `Upgrade ${compareLabel(status.phase)}. No unverified output is accepted.`;
      upgradeStatus.className = succeeded ? "" : "error";
      upgradeActionStatus.textContent = succeeded
        ? "Publication completed without modifying the working Capsule or target release."
        : hostError(status.error || "The upgrade did not report success.");
      upgradeActionStatus.className = succeeded ? "lifecycle-action-status" : "lifecycle-action-status error";
      await invokeHost("acknowledge_upgrade_result", {
        request: { operation_token: operationToken },
      });
      if (activeUpgradeOperationToken === operationToken) activeUpgradeOperationToken = null;
      upgradeCancelButton.disabled = true;
      upgradeExecuteButton.disabled = true;
      upgradeReleaseButton.disabled = false;
      upgradeDestinationButton.disabled = true;
    } catch (error) {
      upgradeActionStatus.textContent = `Could not read the terminal upgrade result: ${hostError(error)}`;
      upgradeActionStatus.className = "lifecycle-action-status error";
    }
  });
}

function oncePerUpgradeOperation(operationToken, finalizer) {
  const existing = upgradeFinalizations.get(operationToken);
  if (existing) return existing;
  const finalization = Promise.resolve().then(finalizer);
  if (upgradeFinalizations.size >= 64) {
    upgradeFinalizations.delete(upgradeFinalizations.keys().next().value);
  }
  upgradeFinalizations.set(operationToken, finalization);
  return finalization;
}

async function pollUpgradeOperation(operationToken) {
  const inFlight = upgradeFinalizations.get(operationToken);
  if (inFlight) return inFlight;
  let status;
  try {
    status = await invokeHost("get_upgrade_operation", {
      request: { operation_token: operationToken },
    });
  } catch (error) {
    const terminal = upgradeFinalizations.get(operationToken);
    if (terminal) return terminal;
    throw error;
  }
  if (status.operation_token !== operationToken) return;
  if (["succeeded", "failed", "cancelled"].includes(status.phase)) {
    await finishUpgradeOperation(operationToken);
    return;
  }
  if (activeUpgradeOperationToken === operationToken) {
    globalThis.setTimeout(() => {
      if (activeUpgradeOperationToken === operationToken) {
        void pollUpgradeOperation(operationToken).catch((error) => {
          upgradeActionStatus.textContent = `Could not poll upgrade status: ${hostError(error)}`;
          upgradeActionStatus.className = "lifecycle-action-status error";
        });
      }
    }, 250);
  }
}

upgradeExecuteButton.addEventListener("click", async () => {
  if (!preparedUpgrade || upgradeExecuteButton.disabled) return;
  const request = {
    plan_id: preparedUpgrade.plan_id,
    confirmation_nonce: preparedUpgrade.confirmation_nonce,
    publisher_key_confirmed: upgradePublisherConfirmation.checked,
    capability_changes_confirmed: upgradeCapabilityConfirmation.checked,
  };
  upgradeExecuteButton.disabled = true;
  upgradeReleaseButton.disabled = true;
  upgradeDestinationButton.disabled = true;
  upgradePrepareButton.disabled = true;
  upgradeActionStatus.textContent = "Consuming the one-use authority and re-verifying both retained inputs…";
  try {
    const status = await invokeHost("execute_upgrade", { request });
    activeUpgradeOperationToken = status.operation_token;
    preparedUpgrade = null;
    upgradeCancelButton.disabled = !status.cancellable;
    upgradeBadge.textContent = compareLabel(status.phase);
    upgradeBadge.className = "badge warn";
    await pollUpgradeOperation(status.operation_token);
  } catch (error) {
    upgradeReleaseButton.disabled = false;
    upgradeActionStatus.textContent = hostError(error);
    upgradeActionStatus.className = "lifecycle-action-status error";
    upgradeBadge.textContent = "Execution rejected";
    upgradeBadge.className = "badge fail";
  }
});

upgradeCancelButton.addEventListener("click", async () => {
  if (!activeUpgradeOperationToken) return;
  upgradeCancelButton.disabled = true;
  try {
    await invokeHost("cancel_upgrade_operation", {
      request: { operation_token: activeUpgradeOperationToken },
    });
    upgradeActionStatus.textContent = "Cancellation requested. The held create-new publication becomes non-cancellable at the no-replace boundary.";
  } catch (error) {
    upgradeActionStatus.textContent = hostError(error);
    upgradeActionStatus.className = "lifecycle-action-status error";
  }
});

async function startSigningPicker(command, message) {
  signingActionStatus.textContent = message;
  signingActionStatus.className = "lifecycle-action-status";
  signingResultWrap.hidden = true;
  try {
    await invokeHost(command);
  } catch (error) {
    signingActionStatus.textContent = String(error);
    signingActionStatus.className = "lifecycle-action-status error";
  }
}

signingKeyButton.addEventListener("click", () => {
  startSigningPicker("select_signing_key_picker", "Choose one local Ed25519 private key. Its bytes stay in Rust memory and are never returned to this page.");
});

signingSourceButton.addEventListener("click", () => {
  startSigningPicker("select_signing_source_picker", "Choose one local SQLite Capsule. It will be inspected and verified without executing embedded assets.");
});

signingOutputButton.addEventListener("click", () => {
  startSigningPicker("select_signing_output_picker", "Choose a new destination. Existing files and the source path are refused.");
});

[signingPublisherId, signingPublisherName].forEach((input) => {
  input.addEventListener("input", () => {
    if (signingSession?.preview && !signingPublisherValuesMatchPreview()) {
      signingActionStatus.textContent = "Publisher identity changed. Prepare a new exact review before signing.";
      signingActionStatus.className = "lifecycle-action-status";
    }
    refreshSigningActions();
  });
});

signingPrepareButton.addEventListener("click", async () => {
  if (signingPrepareButton.disabled) return;
  signingPrepareButton.disabled = true;
  signingActionStatus.textContent = "Verifying the source and preparing an exact same-directory signing copy…";
  signingActionStatus.className = "lifecycle-action-status";
  signingResultWrap.hidden = true;
  try {
    const report = await invokeHost("prepare_signing", {
      request: {
        publisher_id: signingPublisherId.value,
        publisher_name: signingPublisherName.value,
      },
    });
    renderSigningSession(report);
    signingActionStatus.textContent = "Exact application digest prepared. Review every field before enabling the final signature.";
  } catch (error) {
    signingActionStatus.textContent = String(error);
    signingActionStatus.className = "lifecycle-action-status error";
    try { renderSigningSession(await invokeHost("signing_status")); } catch (_) { /* retain local status */ }
  }
});

signingConfirmation.addEventListener("change", refreshSigningActions);

signingExecuteButton.addEventListener("click", async () => {
  if (signingExecuteButton.disabled || !signingSession?.key || !signingSession?.preview) return;
  signingExecuteButton.disabled = true;
  signingActionStatus.textContent = "Signing the reviewed digest, reopening the new file, and verifying the resulting signature…";
  signingActionStatus.className = "lifecycle-action-status";
  try {
    const result = await invokeHost("execute_signing", {
      request: {
        confirmation_key_id: signingSession.key.key_id,
        confirmation_application_digest: signingSession.preview.application_digest,
      },
    });
    signingResult.textContent = JSON.stringify(result, null, 2);
    signingResultWrap.hidden = false;
    signingActionStatus.textContent = `Signed and independently verified ${result.output} (${result.output_bytes.toLocaleString()} bytes). Publisher trust remains a separate host-local decision.`;
    renderSigningSession(await invokeHost("signing_status"));
    signingResult.focus();
  } catch (error) {
    signingActionStatus.textContent = String(error);
    signingActionStatus.className = "lifecycle-action-status error";
    try { renderSigningSession(await invokeHost("signing_status")); } catch (_) { /* retain failure */ }
  }
});

signingClearButton.addEventListener("click", async () => {
  if (signingClearButton.disabled) return;
  signingActionStatus.textContent = "Forgetting the use-once key and removing any prepared temporary copy…";
  signingActionStatus.className = "lifecycle-action-status";
  try {
    renderSigningSession(await invokeHost("clear_signing_session"));
    signingActionStatus.textContent = "Use-once signing session cleared. No private key is retained by the host.";
  } catch (error) {
    signingActionStatus.textContent = String(error);
    signingActionStatus.className = "lifecycle-action-status error";
  }
});

actions.addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-action]");
  if (!button || button.disabled) return;
  actions.querySelectorAll("button").forEach((item) => { item.disabled = true; });
  actionStatus.textContent = "Applying the host-local decision…";
  try {
    const selectionId = currentReport?.selection_id ?? currentReport?.capsule?.overview?.selection_id;
    if (!selectionId) throw new Error("The reviewed Capsule selection is no longer current.");
    const report = await invokeHost("first_open_decide", {
      selectionId,
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

invokeHost("signing_status")
  .then(renderSigningSession)
  .catch((error) => {
    signingStatus.textContent = `Publisher-signing status unavailable: ${String(error)}`;
    signingBadge.className = "badge fail";
    signingBadge.textContent = "Unavailable";
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
    if (event.payload.kind.startsWith("signing-")) {
      signingActionStatus.textContent = event.payload.message;
      signingActionStatus.className = event.payload.kind.endsWith("error")
        ? "lifecycle-action-status error"
        : "lifecycle-action-status";
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
  listen("signing-status", (event) => renderSigningSession(event.payload)).catch((error) => {
    signingStatus.textContent = `Publisher-signing event channel unavailable: ${String(error)}`;
    signingBadge.className = "badge fail";
    signingBadge.textContent = "Unavailable";
  });
  listen("capsule-copy-progress-v1", (event) => {
    const progress = event.payload;
    if (progress?.profile !== "org.sqlite-capsule.copy-progress/1"
      || progress.operation_id !== activeCopyOperationId) return;
    const phaseLabel = String(progress.phase || "working").replaceAll("-", " ");
    copyStatus.textContent = `Create-new operation · ${phaseLabel} · source remains read-only`;
    copyBadge.textContent = phaseLabel;
    copyBadge.className = `badge ${progress.phase === "succeeded" ? "ok" : progress.phase === "failed" ? "fail" : "warn"}`;
    copyClearButton.disabled = !progress.cancellable;
    if (["succeeded", "failed", "cancelled"].includes(progress.phase)) {
      void finishCopyOperation(progress.operation_id);
    }
  }).catch((error) => {
    copyActionStatus.textContent = `Copy progress channel unavailable: ${hostError(error)}`;
    copyActionStatus.className = "lifecycle-action-status error";
  });
  listen("capsule-reconcile-progress-v1", (event) => {
    const progress = event.payload;
    if (progress?.profile !== "org.sqlite-capsule.tauri-reconcile-status/1"
      || progress.operation_token !== activeReconcileOperationToken) return;
    const phaseLabel = compareLabel(progress.phase);
    reconcileBadge.textContent = phaseLabel;
    reconcileBadge.className = `badge ${progress.phase === "succeeded" ? "ok" : progress.phase === "failed" ? "fail" : "warn"}`;
    reconcileStatus.textContent = `Create-new reconciliation · ${phaseLabel} · both inputs remain pinned read-only`;
    reconcileCancelButton.disabled = !progress.cancellable;
    if (["succeeded", "failed", "cancelled"].includes(progress.phase)) {
      void finishReconcileOperation(progress.operation_token);
    }
  }).catch((error) => {
    reconcileStatus.textContent = `Reconciliation progress channel unavailable: ${hostError(error)}`;
    reconcileStatus.className = "lifecycle-action-status error";
  });
  listen("capsule-upgrade-progress-v1", (event) => {
    const progress = event.payload;
    if (progress?.profile !== "org.sqlite-capsule.upgrade-progress/1"
      || progress.operation_token !== activeUpgradeOperationToken) return;
    const phaseLabel = compareLabel(progress.phase);
    upgradeBadge.textContent = phaseLabel;
    upgradeBadge.className = `badge ${progress.phase === "succeeded" ? "ok" : progress.phase === "failed" ? "fail" : "warn"}`;
    upgradeStatus.textContent = `Create-new application upgrade · ${phaseLabel} · both inputs remain pinned read-only`;
    upgradeCancelButton.disabled = !progress.cancellable;
    if (["succeeded", "failed", "cancelled"].includes(progress.phase)) {
      void finishUpgradeOperation(progress.operation_token);
    }
  }).catch((error) => {
    upgradeActionStatus.textContent = `Upgrade progress channel unavailable: ${hostError(error)}`;
    upgradeActionStatus.className = "lifecycle-action-status error";
  });
}

async function chooseCapsule(button, statusTarget) {
  button.disabled = true;
  statusTarget.textContent = "Choose one local SQLite Capsule. Its content will be inspected before any code runs.";
  statusTarget.className = "action-status";
  try {
    await invokeHost("open_capsule_picker");
  } catch (error) {
    statusTarget.textContent = String(error);
    statusTarget.className = "action-status error";
  } finally {
    button.disabled = false;
  }
}

openButton.addEventListener("click", () => chooseCapsule(openButton, openStatus));
cabinetOpenButton.addEventListener("click", () => chooseCapsule(cabinetOpenButton, cabinetOpenStatus));
reviewCapabilitiesButton.addEventListener("click", () => {
  if (reviewCapabilitiesButton.disabled) return;
  const capsule = currentReport?.capsule;
  if (capsule?.decision?.executable_allowed) {
    reviewCapabilitiesButton.disabled = true;
    invokeHost("open_selected_capsule", {
      selectionId: capsule.overview.selection_id,
    }).then(renderReport).catch((error) => {
      openStatus.textContent = `Open failed closed: ${hostError(error)}`;
      openStatus.className = "action-status error";
      reviewCapabilitiesButton.disabled = false;
    });
  } else {
    selectPage("capabilities", { focus: false });
    promptTitle.focus();
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

recoverSelectedButton.addEventListener("click", async () => {
  if (recoverSelectedButton.disabled) return;
  recoverSelectedButton.disabled = true;
  lifecycleActionStatus.textContent = "Running explicit SQLite rollback-journal recovery, then inspecting again with application assets locked…";
  lifecycleActionStatus.className = "lifecycle-action-status";
  try {
    const selectionId = currentReport?.selection_id ?? currentReport?.capsule?.overview?.selection_id;
    if (!selectionId) throw new Error("The reviewed Capsule selection is no longer current.");
    renderReport(await invokeHost("recover_selected_capsule", { selectionId }));
  } catch (error) {
    lifecycleActionStatus.textContent = `Recovery failed closed: ${hostError(error)}`;
    lifecycleActionStatus.className = "lifecycle-action-status error";
    recoverSelectedButton.disabled = false;
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
