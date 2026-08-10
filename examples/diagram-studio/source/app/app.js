(() => {
  "use strict";

  const SVG_NS = "http://www.w3.org/2000/svg";
  const DIAGRAM_ID = "diagram-main";
  const geometry = globalThis.DiagramStudioGeometry;
  const interchange = globalThis.DiagramStudioInterchange;
  const MIN_VIEW_WIDTH = 260;
  const MAX_VIEW_WIDTH = 5200;

  const state = {
    manifest: null,
    exportProfile: "editable",
    token: null,
    diagram: null,
    nodes: [],
    edges: [],
    scenes: [],
    layers: [],
    groups: [],
    history: null,
    historyBusy: false,
    selectedType: null,
    selectedId: null,
    selectedNodeIds: [],
    mode: "select",
    connectSourceId: null,
    viewBox: { x: 0, y: 0, width: 2200, height: 1250 },
    homeView: null,
    sceneIndex: -1,
    focusIds: null,
    sceneOverrides: new Map(),
    drag: null,
    resize: null,
    pan: null,
    marquee: null,
    gestureFrame: null,
    pointerCoordinateFrame: null,
    pointerClient: null,
    animationFrame: null,
    toastTimer: null,
    suppressClickUntil: 0,
    presenting: false,
  };

  const elements = {
    app: document.getElementById("app"),
    loading: document.getElementById("loading-screen"),
    title: document.getElementById("diagram-title"),
    description: document.getElementById("diagram-description"),
    saveState: document.getElementById("save-state"),
    shapePicker: document.getElementById("shape-picker"),
    layoutPicker: document.getElementById("layout-picker"),
    layoutPreview: document.getElementById("layout-preview"),
    copySelection: document.getElementById("copy-selection"),
    pasteSelection: document.getElementById("paste-selection"),
    importJson: document.getElementById("import-json"),
    importFile: document.getElementById("import-file"),
    exportPicker: document.getElementById("export-picker"),
    exportDiagram: document.getElementById("export-diagram"),
    undo: document.getElementById("undo"),
    redo: document.getElementById("redo"),
    addNode: document.getElementById("add-node"),
    connectMode: document.getElementById("connect-mode"),
    fitView: document.getElementById("fit-view"),
    present: document.getElementById("present"),
    sceneCount: document.getElementById("scene-count"),
    sceneList: document.getElementById("scene-list"),
    sceneAdd: document.getElementById("scene-add"),
    sceneRename: document.getElementById("scene-rename"),
    sceneCapture: document.getElementById("scene-capture"),
    sceneDuplicate: document.getElementById("scene-duplicate"),
    sceneUp: document.getElementById("scene-up"),
    sceneDown: document.getElementById("scene-down"),
    sceneDelete: document.getElementById("scene-delete"),
    layerList: document.getElementById("layer-list"),
    canvasShell: document.querySelector(".canvas-shell"),
    canvas: document.getElementById("diagram-canvas"),
    background: document.getElementById("canvas-background"),
    elementLayer: document.getElementById("element-layer"),
    marqueeLayer: document.getElementById("marquee-layer"),
    coordinates: document.getElementById("coordinates"),
    modeHint: document.getElementById("mode-hint"),
    inspector: document.getElementById("inspector"),
    toast: document.getElementById("toast"),
    presentationOverlay: document.getElementById("presentation-overlay"),
    presentationIndex: document.getElementById("presentation-index"),
    presentationTitle: document.getElementById("presentation-title"),
    presentationNarrative: document.getElementById("presentation-narrative"),
    previousScene: document.getElementById("previous-scene"),
    nextScene: document.getElementById("next-scene"),
    exitPresentation: document.getElementById("exit-presentation"),
  };

  const defaultStyles = {
    container: {
      fill: "#111827",
      fillOpacity: 0.38,
      stroke: "#64748b",
      strokeWidth: 2,
      dash: "10 8",
      text: "#e2e8f0",
      accent: "#94a3b8",
      radius: 28,
    },
    prompt: {
      fill: "#312e81",
      fillOpacity: 1,
      stroke: "#818cf8",
      strokeWidth: 2,
      text: "#eef2ff",
      accent: "#a5b4fc",
      radius: 22,
    },
    instruction: {
      fill: "#78350f",
      fillOpacity: 1,
      stroke: "#f59e0b",
      strokeWidth: 2,
      text: "#fffbeb",
      accent: "#fbbf24",
      radius: 22,
    },
    runtime: {
      fill: "#0c4a6e",
      fillOpacity: 1,
      stroke: "#38bdf8",
      strokeWidth: 2,
      text: "#f0f9ff",
      accent: "#7dd3fc",
      radius: 24,
    },
    component: {
      fill: "#1e293b",
      fillOpacity: 1,
      stroke: "#94a3b8",
      strokeWidth: 2,
      text: "#f8fafc",
      accent: "#cbd5e1",
      radius: 18,
    },
    output: {
      fill: "#064e3b",
      fillOpacity: 1,
      stroke: "#2dd4bf",
      strokeWidth: 2,
      text: "#f0fdfa",
      accent: "#5eead4",
      radius: 28,
    },
    export: {
      fill: "#500724",
      fillOpacity: 1,
      stroke: "#fb7185",
      strokeWidth: 2,
      text: "#fff1f2",
      accent: "#fda4af",
      radius: 28,
    },
    note: {
      fill: "#172554",
      fillOpacity: 1,
      stroke: "#60a5fa",
      strokeWidth: 2,
      text: "#eff6ff",
      accent: "#93c5fd",
      radius: 18,
    },
  };

  function svgElement(name, attributes = {}) {
    const element = document.createElementNS(SVG_NS, name);
    for (const [key, value] of Object.entries(attributes)) {
      if (value !== undefined && value !== null) {
        element.setAttribute(key, String(value));
      }
    }
    return element;
  }

  function escapeHtml(value) {
    return String(value ?? "")
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#039;");
  }

  function clamp(value, minimum, maximum) {
    return Math.min(maximum, Math.max(minimum, value));
  }

  function toFiniteNumber(value, fallback = 0) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : fallback;
  }

  function safePaint(value, fallback) {
    if (typeof value !== "string") return fallback;
    const candidate = value.trim();
    const allowed = /^(#[0-9a-fA-F]{3,8}|rgba?\([\d\s.,%+-]+\)|hsla?\([\d\s.,%+\-a-zA-Z]+\)|transparent|currentColor)$/;
    return allowed.test(candidate) ? candidate : fallback;
  }

  function safeDash(value) {
    if (typeof value !== "string") return "";
    return /^[\d.\s,]+$/.test(value.trim()) ? value.trim() : "";
  }

  function canEdit() {
    return state.exportProfile === "editable";
  }

  function canUseInterchange() {
    return state.exportProfile !== "view";
  }

  function applyExportProfile() {
    const requested = state.manifest?.export_profile;
    state.exportProfile = ["view", "interactive", "editable"].includes(requested) ? requested : "editable";
    document.body.dataset.exportProfile = state.exportProfile;
    document.body.classList.toggle("profile-editable", canEdit());
    document.body.classList.toggle("profile-interactive", state.exportProfile === "interactive");
    document.body.classList.toggle("profile-view", state.exportProfile === "view");

    const editableControls = [
      elements.undo,
      elements.redo,
      elements.shapePicker.closest("label"),
      elements.layoutPicker.closest("label"),
      elements.layoutPreview,
      elements.pasteSelection,
      elements.importJson,
      elements.addNode,
      elements.connectMode,
      document.querySelector(".scene-authoring"),
    ];
    for (const control of editableControls) {
      if (control) control.hidden = !canEdit();
    }

    const interchangeControls = [
      elements.copySelection,
      elements.exportPicker.closest("label"),
      elements.exportDiagram,
    ];
    for (const control of interchangeControls) {
      if (control) control.hidden = !canUseInterchange();
    }

    const inspectorPanel = document.querySelector(".inspector-panel");
    if (inspectorPanel) inspectorPanel.hidden = state.exportProfile === "view";
    elements.inspector.inert = !canEdit();
    elements.inspector.setAttribute("aria-disabled", String(!canEdit()));

    const footerHint = document.querySelector(".canvas-footer span:last-child");
    if (footerHint) {
      footerHint.textContent = canEdit()
        ? "Drag canvas to pan · Shift-drag to marquee · wheel to zoom · double-click node to rename"
        : "Drag canvas to pan · Shift-drag to select · wheel to zoom · use scenes for the guided view";
    }
    const keyboardCard = document.querySelector(".keyboard-card");
    if (keyboardCard && !canEdit()) {
      keyboardCard.innerHTML = `<span><kbd>F</kbd> fit</span><span><kbd>P</kbd> present</span><span>Read-only profile</span>`;
    }
  }

  function nodeStyle(node) {
    const base = defaultStyles[node.kind] || defaultStyles.component;
    const custom = node.style_json && typeof node.style_json === "object" ? node.style_json : {};
    return {
      fill: safePaint(custom.fill, base.fill),
      fillOpacity: clamp(toFiniteNumber(custom.fillOpacity, base.fillOpacity), 0, 1),
      stroke: safePaint(custom.stroke, base.stroke),
      strokeWidth: clamp(toFiniteNumber(custom.strokeWidth, base.strokeWidth), 1, 8),
      dash: safeDash(custom.dash || base.dash || ""),
      text: safePaint(custom.text, base.text),
      accent: safePaint(custom.accent, base.accent),
      radius: clamp(toFiniteNumber(custom.radius, base.radius), 0, 48),
    };
  }

  function edgeStyle(edge) {
    const custom = edge.style_json && typeof edge.style_json === "object" ? edge.style_json : {};
    return {
      stroke: safePaint(custom.stroke, "#64748b"),
      width: clamp(toFiniteNumber(custom.width, 2.5), 1, 8),
      curve: clamp(toFiniteNumber(custom.curve, 0), -0.45, 0.45),
      dash: safeDash(custom.dash || ""),
    };
  }

  function dataFor(node) {
    return node.data_json && typeof node.data_json === "object" ? node.data_json : {};
  }

  function uid(prefix) {
    if (globalThis.crypto?.randomUUID) return `${prefix}-${globalThis.crypto.randomUUID()}`;
    return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  }

  async function getManifest() {
    if (!globalThis.SQLiteCapsuleClient) throw new Error("SQLite Capsule client is unavailable");
    state.manifest = await globalThis.SQLiteCapsuleClient.manifest();
  }

  async function readEndpoint(name, parameters = {}) {
    return globalThis.SQLiteCapsuleClient.read(name, parameters);
  }

  async function writeEndpoint(name, parameters) {
    if (!canEdit()) throw new Error(`The ${state.exportProfile} export profile is read-only`);
    setSaveState("Saving to SQLite", "saving");
    try {
      const result = await globalThis.SQLiteCapsuleClient.write(name, parameters);
      setSaveState("Saved in SQLite", "saved");
      return result;
    } catch (error) {
      setSaveState("Save failed", "error");
      throw error;
    }
  }

  async function boot() {
    bindStaticEvents();
    try {
      await getManifest();
      applyExportProfile();
      const [diagram, nodes, edges, scenes, layers, groups, history] = await Promise.all([
        readEndpoint("diagram.get", { diagram_id: DIAGRAM_ID }),
        readEndpoint("diagram.nodes", { diagram_id: DIAGRAM_ID }),
        readEndpoint("diagram.edges", { diagram_id: DIAGRAM_ID }),
        readEndpoint("diagram.scenes", { diagram_id: DIAGRAM_ID }),
        readEndpoint("diagram.layers", { diagram_id: DIAGRAM_ID }),
        readEndpoint("diagram.groups", { diagram_id: DIAGRAM_ID }),
        readEndpoint("diagram.history", { diagram_id: DIAGRAM_ID }),
      ]);
      if (!diagram) throw new Error(`Diagram ${DIAGRAM_ID} is missing`);
      state.diagram = diagram;
      state.nodes = nodes;
      state.edges = edges;
      state.scenes = scenes;
      state.layers = layers;
      state.groups = groups;
      state.history = history;
      state.homeView = calculateBounds(110);
      state.viewBox = { ...state.homeView };
      elements.title.textContent = diagram.title;
      elements.description.textContent = diagram.description;
      document.title = `${diagram.title} — SQLite Capsule`;
      renderAll();
      setSaveState(canEdit() ? "Local · SQLite" : `${state.exportProfile} · read-only`, "saved");
      elements.app.setAttribute("aria-busy", "false");
      requestAnimationFrame(() => elements.loading.classList.add("is-hidden"));
      showToast("Application, diagram, scenes, and APIs loaded from one SQLite file.");
    } catch (error) {
      showBootError(error);
    }
  }

  async function refreshHistory() {
    state.history = await readEndpoint("diagram.history", { diagram_id: DIAGRAM_ID });
    updateHistoryUI();
    return state.history;
  }

  function updateHistoryUI() {
    const history = state.history;
    const canUndo = Boolean(history?.undo_operation_id && history?.undo_endpoint);
    const canRedo = Boolean(history?.redo_operation_id && history?.redo_endpoint);
    elements.undo.disabled = !canEdit() || state.historyBusy || !canUndo;
    elements.redo.disabled = !canEdit() || state.historyBusy || !canRedo;
    elements.undo.title = canUndo ? `Undo: ${history.undo_summary} (Ctrl+Z)` : "Nothing to undo";
    elements.redo.title = canRedo ? `Redo: ${history.redo_summary} (Ctrl+Shift+Z or Ctrl+Y)` : "Nothing to redo";
  }

  async function executeCommand(name, parameters) {
    if (!state.history) throw new Error("Diagram history is unavailable");
    const result = await writeEndpoint(name, {
      ...parameters,
      operation_id: uid("operation"),
      diagram_id: DIAGRAM_ID,
      expected_cursor: state.history.cursor,
    });
    await refreshHistory();
    return result;
  }

  async function reloadDiagramModel() {
    const [diagram, nodes, edges, layers, groups, history] = await Promise.all([
      readEndpoint("diagram.get", { diagram_id: DIAGRAM_ID }),
      readEndpoint("diagram.nodes", { diagram_id: DIAGRAM_ID }),
      readEndpoint("diagram.edges", { diagram_id: DIAGRAM_ID }),
      readEndpoint("diagram.layers", { diagram_id: DIAGRAM_ID }),
      readEndpoint("diagram.groups", { diagram_id: DIAGRAM_ID }),
      readEndpoint("diagram.history", { diagram_id: DIAGRAM_ID }),
    ]);
    state.diagram = diagram;
    state.nodes = nodes;
    state.edges = edges;
    state.layers = layers;
    state.groups = groups;
    state.history = history;
    state.selectedNodeIds = state.selectedNodeIds.filter((id) => nodes.some((item) => item.id === id));
    const selected = state.selectedType === "node"
      ? state.selectedNodeIds.length > 0
      : state.selectedType === "edge" && edges.some((item) => item.id === state.selectedId);
    if (!selected) {
      state.selectedType = null;
      state.selectedId = null;
      state.selectedNodeIds = [];
    }
    elements.title.textContent = diagram.title;
    document.title = `${diagram.title} — SQLite Capsule`;
    renderAll();
    requestAnimationFrame(focusSelectedObject);
  }

  function focusSelectedObject() {
    if (!state.selectedId) {
      elements.canvas.focus();
      return;
    }
    const selector = `[data-id="${CSS.escape(state.selectedId)}"]`;
    elements.elementLayer.querySelector(`.${state.selectedType}${selector}`)?.focus();
  }

  async function moveHistory(direction) {
    if (state.historyBusy || !state.history) return;
    const undoing = direction === "undo";
    const endpoint = undoing ? state.history.undo_endpoint : state.history.redo_endpoint;
    const operationId = undoing ? state.history.undo_operation_id : state.history.redo_operation_id;
    const summary = undoing ? state.history.undo_summary : state.history.redo_summary;
    if (!endpoint || !operationId) return;
    state.historyBusy = true;
    updateHistoryUI();
    try {
      await writeEndpoint(endpoint, {
        operation_id: operationId,
        diagram_id: DIAGRAM_ID,
        expected_cursor: state.history.cursor,
      });
      await reloadDiagramModel();
      showToast(`${undoing ? "Undid" : "Redid"}: ${summary}.`);
    } catch (error) {
      await refreshHistory().catch(() => {});
      showToast(error.message, true);
    } finally {
      state.historyBusy = false;
      updateHistoryUI();
    }
  }

  function showBootError(error) {
    console.error(error);
    const card = elements.loading.querySelector(".loading-card");
    card.querySelector("h2").textContent = "The capsule could not be opened";
    card.querySelector("p").textContent = error instanceof Error ? error.message : String(error);
    card.querySelector(".loading-line").remove();
    setSaveState("Open failed", "error");
  }

  function bindStaticEvents() {
    elements.undo.addEventListener("click", () => moveHistory("undo"));
    elements.redo.addEventListener("click", () => moveHistory("redo"));
    elements.addNode.addEventListener("click", addNode);
    elements.layoutPreview.addEventListener("click", previewLayout);
    elements.copySelection.addEventListener("click", copySelection);
    elements.pasteSelection.addEventListener("click", pasteSelection);
    elements.importJson.addEventListener("click", () => elements.importFile.click());
    elements.importFile.addEventListener("change", importSelectedFile);
    elements.exportDiagram.addEventListener("click", exportDiagram);
    elements.connectMode.addEventListener("click", toggleConnectMode);
    elements.fitView.addEventListener("click", () => {
      clearSceneFocus();
      animateViewBox(calculateBounds(110));
    });
    elements.present.addEventListener("click", enterPresentation);
    elements.previousScene.addEventListener("click", () => moveScene(-1));
    elements.nextScene.addEventListener("click", () => moveScene(1));
    elements.exitPresentation.addEventListener("click", exitPresentation);
    elements.sceneAdd.addEventListener("click", () => mutateScenes("add"));
    elements.sceneRename.addEventListener("click", () => mutateScenes("rename"));
    elements.sceneCapture.addEventListener("click", () => mutateScenes("capture"));
    elements.sceneDuplicate.addEventListener("click", () => mutateScenes("duplicate"));
    elements.sceneUp.addEventListener("click", () => mutateScenes("up"));
    elements.sceneDown.addEventListener("click", () => mutateScenes("down"));
    elements.sceneDelete.addEventListener("click", () => mutateScenes("delete"));
    elements.layerList.addEventListener("click", (event) => {
      const button = event.target.closest?.("button[data-action]");
      const row = button?.closest?.(".layer-row");
      if (!button || !row) return;
      if (["up", "down"].includes(button.dataset.action)) reorderSemanticLayer(row.dataset.id, button.dataset.action);
      else updateLayerState(row.dataset.id, button.dataset.action);
    });

    elements.canvas.addEventListener("pointerdown", onCanvasPointerDown);
    elements.canvas.addEventListener("pointermove", onCanvasPointerMove);
    elements.canvas.addEventListener("pointerup", onCanvasPointerUp);
    elements.canvas.addEventListener("pointercancel", onCanvasPointerUp);
    elements.canvas.addEventListener("wheel", onCanvasWheel, { passive: false });
    elements.canvas.addEventListener("click", onCanvasClick);
    elements.canvas.addEventListener("mousemove", updatePointerCoordinates);
    elements.canvas.addEventListener("contextmenu", (event) => event.preventDefault());

    document.addEventListener("keydown", onKeyDown);
  }

  function renderAll() {
    applyViewBox(state.viewBox);
    renderSceneList();
    renderLayerList();
    renderDiagram();
    renderInspector();
    updateModeUI();
    updatePresentationOverlay();
    updateHistoryUI();
  }

  function renderDiagram() {
    elements.elementLayer.replaceChildren();
    const orderedLayers = [...state.layers]
      .filter((layer) => layer.visible !== 0)
      .sort((a, b) => a.position - b.position || a.id.localeCompare(b.id));
    for (const layer of orderedLayers) {
      const group = svgElement("g", { class: "semantic-canvas-layer", "data-layer-id": layer.id, "aria-label": `${layer.name} layer` });
      renderEdges(group, layer.id);
      renderNodes(group, layer.id);
      elements.elementLayer.append(group);
    }
    updateCoordinatesLabel();
  }

  function nodeElement(nodeId) {
    return elements.elementLayer.querySelector(`.node[data-id="${CSS.escape(nodeId)}"]`);
  }

  function previewConnectedEdges(nodeIds) {
    const changed = new Set(nodeIds);
    for (const edge of state.edges) {
      if (!changed.has(edge.source_id) && !changed.has(edge.target_id)) continue;
      const sourceNode = state.nodes.find((node) => node.id === edge.source_id);
      const targetNode = state.nodes.find((node) => node.id === edge.target_id);
      if (!sourceNode || !targetNode) continue;
      const source = effectiveNode(sourceNode);
      const target = effectiveNode(targetNode);
      const style = edgeStyle(edge);
      const pathGeometry = edgeGeometry(source, target, style.curve);
      const group = elements.elementLayer.querySelector(`.edge[data-id="${CSS.escape(edge.id)}"]`);
      if (!group) continue;
      group.querySelectorAll(".edge-visible, .edge-hit").forEach((path) => path.setAttribute("d", pathGeometry.path));
    }
  }

  function previewNodeSize(node) {
    const group = nodeElement(node.id);
    if (!group) return;
    group.classList.add("is-gesture-preview");
    const data = dataFor(node);
    const style = nodeStyle(node);
    const shape = data.shape || (node.kind === "container" ? "container" : "rounded-rectangle");
    const body = group.querySelector(".node-body");
    if (body?.tagName.toLowerCase() === "path") {
      body.setAttribute("d", geometry.shapePath(shape, node.width, node.height));
    } else if (body) {
      body.setAttribute("width", node.width);
      body.setAttribute("height", node.height);
      body.setAttribute("rx", shape === "rectangle" ? 0 : shape === "pill" ? node.height / 2 : style.radius);
    }
    const accent = group.querySelector(".node-accent");
    if (node.kind === "container") accent?.setAttribute("x2", Math.max(34, node.width - 34));
    else accent?.setAttribute("height", node.height);
    const handle = group.querySelector(".resize-handle");
    handle?.setAttribute("x", node.width - 8);
    handle?.setAttribute("y", node.height - 8);
  }

  function previewInspectorNode(node) {
    const x = document.getElementById("node-x-value");
    const y = document.getElementById("node-y-value");
    const width = document.getElementById("node-width-input");
    const height = document.getElementById("node-height-input");
    if (x) x.textContent = Math.round(node.x);
    if (y) y.textContent = Math.round(node.y);
    if (width) width.value = Math.round(node.width);
    if (height) height.value = Math.round(node.height);
  }

  function applyGesturePreview() {
    state.gestureFrame = null;
    if (state.resize?.pending) {
      const node = state.nodes.find((item) => item.id === state.resize.nodeId);
      if (!node) return;
      node.width = state.resize.pending.width;
      node.height = state.resize.pending.height;
      previewNodeSize(node);
      previewConnectedEdges([node.id]);
      previewInspectorNode(node);
      return;
    }
    if (state.drag?.pending) {
      const movedIds = [];
      for (const start of state.drag.starts) {
        const node = state.nodes.find((item) => item.id === start.id);
        if (!node) continue;
        node.x = start.x + state.drag.pending.dx;
        node.y = start.y + state.drag.pending.dy;
        const group = nodeElement(node.id);
        group?.classList.add("is-gesture-preview");
        group?.setAttribute("transform", `translate(${node.x} ${node.y})`);
        movedIds.push(node.id);
      }
      previewConnectedEdges(movedIds);
      if (movedIds.length === 1) {
        const node = state.nodes.find((item) => item.id === movedIds[0]);
        if (node) previewInspectorNode(node);
      }
    }
  }

  function scheduleGesturePreview() {
    if (state.gestureFrame !== null) return;
    state.gestureFrame = requestAnimationFrame(applyGesturePreview);
  }

  function flushGesturePreview() {
    if (state.gestureFrame !== null) {
      cancelAnimationFrame(state.gestureFrame);
      state.gestureFrame = null;
    }
    applyGesturePreview();
  }

  function cancelGesturePreview() {
    if (state.gestureFrame !== null) cancelAnimationFrame(state.gestureFrame);
    state.gestureFrame = null;
  }

  function renderEdges(parent, layerId) {
    const nodeMap = new Map(state.nodes.map((node) => [node.id, effectiveNode(node)]).filter(([, node]) => !node._sceneHidden));
    for (const edge of state.edges.filter((item) => item.layer_id === layerId)) {
      if (!layerVisible(edge.layer_id)) continue;
      const source = nodeMap.get(edge.source_id);
      const target = nodeMap.get(edge.target_id);
      if (!source || !target || !layerVisible(source.layer_id) || !layerVisible(target.layer_id)) continue;
      const style = edgeStyle(edge);
      const obstacles = state.nodes.filter((node) => node.id !== source.id && node.id !== target.id && layerVisible(node.layer_id));
      const route = edge.route_mode === "direct"
        ? null
        : geometry.routeOrthogonal(source, target, obstacles, {
            sourcePort: edge.source_port === "auto" ? undefined : edge.source_port,
            targetPort: edge.target_port === "auto" ? undefined : edge.target_port,
          });
      const pathGeometry = route
        ? { path: route.path, label: route.points[Math.floor(route.points.length / 2)] }
        : edgeGeometry(source, target, style.curve);
      const group = svgElement("g", { class: "edge", "data-id": edge.id, tabindex: 0, role: "button", "aria-label": `Connector from ${source.label} to ${target.label}` });
      if (route?.fallback) {
        group.classList.add("is-route-fallback");
        group.append(svgElement("title"));
        group.lastChild.textContent = "No obstacle-free orthogonal route; showing deterministic fallback";
      }
      if (state.selectedType === "edge" && state.selectedId === edge.id) {
        group.classList.add("is-selected");
      }
      if (isEdgeDimmed(edge)) group.classList.add("is-focus-dim");

      const visible = svgElement("path", {
        class: "edge-visible",
        d: pathGeometry.path,
        stroke: style.stroke,
        "stroke-width": style.width,
        "stroke-dasharray": style.dash || null,
      });
      const hit = svgElement("path", { class: "edge-hit", d: pathGeometry.path });
      hit.addEventListener("click", (event) => {
        event.stopPropagation();
        if (Date.now() < state.suppressClickUntil) return;
        selectObject("edge", edge.id);
      });
      group.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          selectObject("edge", edge.id);
        }
      });
      group.append(visible, hit);

      if (edge.label) {
        const labelWidth = Math.max(48, edge.label.length * 6.2 + 18);
        const background = svgElement("rect", {
          class: "edge-label-bg",
          x: pathGeometry.label.x - labelWidth / 2,
          y: pathGeometry.label.y - 12,
          width: labelWidth,
          height: 23,
          rx: 10,
        });
        const text = svgElement("text", {
          class: "edge-label",
          x: pathGeometry.label.x,
          y: pathGeometry.label.y + 4,
        });
        text.textContent = edge.label;
        group.append(background, text);
      }
      if (route && state.selectedType === "edge" && state.selectedId === edge.id) {
        group.append(
          svgElement("circle", { class: "connector-handle", cx: route.points[0].x, cy: route.points[0].y, r: 8 }),
          svgElement("circle", { class: "connector-handle", cx: route.points.at(-1).x, cy: route.points.at(-1).y, r: 8 }),
        );
      }
      parent.append(group);
    }
  }

  function renderNodes(parent, layerId) {
    const ordered = [...state.nodes]
      .filter((node) => node.layer_id === layerId)
      .filter((node) => layerVisible(node.layer_id))
      .filter((node) => !effectiveNode(node)._sceneHidden)
      .sort((a, b) => layerPosition(a.layer_id) - layerPosition(b.layer_id) || a.z_index - b.z_index || a.id.localeCompare(b.id));
    for (const node of ordered) {
      const viewNode = effectiveNode(node);
      const style = nodeStyle(viewNode);
      const data = dataFor(viewNode);
      const group = svgElement("g", {
        class: `node kind-${node.kind}`,
        transform: `translate(${viewNode.x} ${viewNode.y})`,
        "data-id": node.id,
        tabindex: 0,
        role: "button",
        "aria-label": `${node.label}, ${node.kind} node`,
        "aria-pressed": state.selectedNodeIds.includes(node.id) ? "true" : "false",
      });
      if (!isNodeLocked(node)) group.classList.add("is-draggable");
      if (state.selectedType === "node" && state.selectedNodeIds.includes(node.id)) {
        group.classList.add("is-selected");
      }
      if (state.connectSourceId === node.id) group.classList.add("is-connect-source");
      if (isNodeDimmed(node.id)) group.classList.add("is-focus-dim");

      const title = svgElement("title");
      title.textContent = [node.label, data.description].filter(Boolean).join(" — ");
      group.append(title);

      const shape = data.shape || (node.kind === "container" ? "container" : "rounded-rectangle");
      const pathShape = ["ellipse", "diamond", "note"].includes(shape);
      const body = svgElement(pathShape ? "path" : "rect", {
        class: "node-body",
        x: pathShape ? null : 0,
        y: pathShape ? null : 0,
        width: pathShape ? null : viewNode.width,
        height: pathShape ? null : viewNode.height,
        d: pathShape ? geometry.shapePath(shape, viewNode.width, viewNode.height) : null,
        rx: pathShape ? null : shape === "rectangle" ? 0 : shape === "pill" ? viewNode.height / 2 : style.radius,
        fill: style.fill,
        "fill-opacity": style.fillOpacity,
        stroke: style.stroke,
        "stroke-width": style.strokeWidth,
        "stroke-dasharray": style.dash || null,
      });
      group.append(body);

      if (node.kind === "container") {
        const accent = svgElement("line", {
          class: "node-accent",
          x1: 34,
          y1: 96,
          x2: viewNode.width - 34,
          y2: 96,
          stroke: style.accent,
          "stroke-opacity": 0.35,
          "stroke-width": 1.5,
        });
        group.append(accent);
        appendText(group, data.eyebrow || "CONTAINER", 34, 30, "node-eyebrow", style.accent);
        appendWrappedText(group, node.label, 34, 62, viewNode.width - 68, 24, 29, 1, "node-label", style.text);
        appendWrappedText(
          group,
          data.description || "",
          34,
          124,
          viewNode.width - 68,
          13,
          18,
          2,
          "node-description",
          style.text,
        );
      } else {
        const accent = svgElement("rect", {
          class: "node-accent",
          x: 0,
          y: 0,
          width: 7,
          height: viewNode.height,
          rx: Math.min(style.radius, 7),
          fill: style.accent,
          "fill-opacity": 0.95,
        });
        group.append(accent);
        appendText(group, data.eyebrow || node.kind.toUpperCase(), 24, 27, "node-eyebrow", style.accent);
        const labelLines = appendWrappedText(
          group,
          node.label,
          24,
          52,
          viewNode.width - 46,
          viewNode.width >= 360 ? 19 : 17,
          viewNode.width >= 360 ? 23 : 21,
          viewNode.height >= 170 ? 3 : 2,
          "node-label",
          style.text,
        );
        const descriptionY = 58 + labelLines * (viewNode.width >= 360 ? 23 : 21);
        const remaining = viewNode.height - descriptionY - 14;
        const maxDescriptionLines = clamp(Math.floor(remaining / 16), 0, 4);
        if (maxDescriptionLines > 0 && data.description) {
          appendWrappedText(
            group,
            data.description,
            24,
            descriptionY,
            viewNode.width - 46,
            12,
            16,
            maxDescriptionLines,
            "node-description",
            style.text,
          );
        }
      }

      group.addEventListener("pointerdown", (event) => startNodePointer(event, node));
      group.addEventListener("click", (event) => {
        event.stopPropagation();
        if (Date.now() < state.suppressClickUntil) return;
        activateNode(node, event);
      });
      group.addEventListener("dblclick", (event) => {
        event.stopPropagation();
        if (!canEdit() || state.mode !== "select") return;
        const next = window.prompt("Rename node", node.label);
        if (next !== null) renameNode(node, next);
      });
      group.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          selectObject("node", node.id, event.shiftKey);
        }
      });
      if (canEdit() && state.selectedType === "node" && state.selectedNodeIds.length === 1 && state.selectedId === node.id && !isNodeLocked(node)) {
        const handle = svgElement("rect", {
          class: "resize-handle",
          x: viewNode.width - 8,
          y: viewNode.height - 8,
          width: 16,
          height: 16,
          rx: 4,
          "aria-label": `Resize ${node.label}`,
        });
        handle.addEventListener("pointerdown", (event) => startResizePointer(event, node));
        group.append(handle);
      }
      parent.append(group);
    }
  }

  function appendText(parent, value, x, y, className, fill) {
    const text = svgElement("text", { x, y, class: className, fill });
    text.textContent = value;
    parent.append(text);
    return text;
  }

  function appendWrappedText(parent, value, x, y, maxWidth, fontSize, lineHeight, maxLines, className, fill) {
    if (!value || maxLines <= 0) return 0;
    const lines = wrapLines(String(value), maxWidth, fontSize, maxLines);
    const text = svgElement("text", { x, y, class: className, fill });
    lines.forEach((line, index) => {
      const span = svgElement("tspan", { x, dy: index === 0 ? 0 : lineHeight });
      span.textContent = line;
      text.append(span);
    });
    parent.append(text);
    return lines.length;
  }

  function wrapLines(value, maxWidth, fontSize, maxLines) {
    const approximateCharacters = Math.max(5, Math.floor(maxWidth / (fontSize * 0.56)));
    const words = value.trim().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    const lines = [];
    let current = "";
    let truncated = false;
    for (const originalWord of words) {
      const chunks = [];
      let word = originalWord;
      while (word.length > approximateCharacters) {
        chunks.push(word.slice(0, approximateCharacters - 1) + "‑");
        word = word.slice(approximateCharacters - 1);
      }
      chunks.push(word);
      for (const chunk of chunks) {
        const candidate = current ? `${current} ${chunk}` : chunk;
        if (candidate.length <= approximateCharacters) {
          current = candidate;
        } else {
          if (current) lines.push(current);
          current = chunk;
          if (lines.length >= maxLines) {
            truncated = true;
            break;
          }
        }
      }
      if (truncated || lines.length >= maxLines) {
        truncated = true;
        break;
      }
    }
    if (!truncated && current) lines.push(current);
    if (lines.length > maxLines) lines.length = maxLines;
    if (truncated && lines.length) {
      const lastIndex = lines.length - 1;
      const line = lines[lastIndex].replace(/[.,;:!?]?$/, "");
      lines[lastIndex] = `${line.slice(0, Math.max(1, approximateCharacters - 1))}…`;
    }
    return lines;
  }

  function edgeGeometry(source, target, curve) {
    const sourceCenter = centerOf(source);
    const targetCenter = centerOf(target);
    const start = rectangleAnchor(source, targetCenter);
    const end = rectangleAnchor(target, sourceCenter);
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    const length = Math.max(1, Math.hypot(dx, dy));
    const normal = { x: -dy / length, y: dx / length };
    const bend = curve * length;
    const control1 = {
      x: start.x + dx * 0.34 + normal.x * bend,
      y: start.y + dy * 0.34 + normal.y * bend,
    };
    const control2 = {
      x: start.x + dx * 0.66 + normal.x * bend,
      y: start.y + dy * 0.66 + normal.y * bend,
    };
    const label = cubicPoint(start, control1, control2, end, 0.5);
    return {
      path: `M ${start.x} ${start.y} C ${control1.x} ${control1.y}, ${control2.x} ${control2.y}, ${end.x} ${end.y}`,
      label,
    };
  }

  function centerOf(node) {
    return { x: node.x + node.width / 2, y: node.y + node.height / 2 };
  }

  function rectangleAnchor(node, toward) {
    const center = centerOf(node);
    const dx = toward.x - center.x;
    const dy = toward.y - center.y;
    if (Math.abs(dx) < 0.001 && Math.abs(dy) < 0.001) return center;
    const scaleX = Math.abs(dx) < 0.001 ? Infinity : node.width / 2 / Math.abs(dx);
    const scaleY = Math.abs(dy) < 0.001 ? Infinity : node.height / 2 / Math.abs(dy);
    const scale = Math.min(scaleX, scaleY);
    return { x: center.x + dx * scale, y: center.y + dy * scale };
  }

  function cubicPoint(start, c1, c2, end, t) {
    const mt = 1 - t;
    return {
      x: mt ** 3 * start.x + 3 * mt ** 2 * t * c1.x + 3 * mt * t ** 2 * c2.x + t ** 3 * end.x,
      y: mt ** 3 * start.y + 3 * mt ** 2 * t * c1.y + 3 * mt * t ** 2 * c2.y + t ** 3 * end.y,
    };
  }

  function isNodeDimmed(nodeId) {
    return state.focusIds instanceof Set && !state.focusIds.has(nodeId);
  }

  function isEdgeDimmed(edge) {
    return state.focusIds instanceof Set && !(state.focusIds.has(edge.source_id) && state.focusIds.has(edge.target_id));
  }

  function renderSceneList() {
    elements.sceneList.replaceChildren();
    elements.sceneCount.textContent = String(state.scenes.length);
    state.scenes.forEach((scene, index) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "scene-button";
      if (index === state.sceneIndex) button.classList.add("is-active");
      button.innerHTML = `
        <span class="scene-number">${String(index + 1).padStart(2, "0")}</span>
        <span class="scene-copy">
          <strong>${escapeHtml(scene.title)}</strong>
          <span>${escapeHtml(scene.narrative)}</span>
        </span>`;
      button.addEventListener("click", () => applyScene(index));
      elements.sceneList.append(button);
    });
  }

  function layerFor(id) {
    return state.layers.find((layer) => layer.id === id);
  }

  function layerVisible(id) {
    return layerFor(id)?.visible !== 0;
  }

  function layerPosition(id) {
    return layerFor(id)?.position ?? 0;
  }

  function isNodeLocked(node) {
    return Boolean(dataFor(node).locked || layerFor(node.layer_id)?.locked);
  }

  function renderLayerList() {
    elements.layerList.replaceChildren();
    const ordered = [...state.layers].sort((a, b) => a.position - b.position || a.id.localeCompare(b.id));
    for (const [index, layer] of ordered.entries()) {
      const row = document.createElement("div");
      row.className = "layer-row";
      row.dataset.id = layer.id;
      const disabled = canEdit() ? "" : "disabled hidden";
      row.classList.toggle("is-read-only", !canEdit());
      row.innerHTML = `
        <button type="button" data-action="visibility" aria-label="${layer.visible ? "Hide" : "Show"} ${escapeHtml(layer.name)}" ${disabled}>${layer.visible ? "◉" : "○"}</button>
        <div class="layer-copy"><strong>${escapeHtml(layer.name)}</strong><span>${layer.node_count} nodes · ${layer.edge_count} edges</span></div>
        <button type="button" data-action="up" aria-label="Move ${escapeHtml(layer.name)} layer earlier" ${disabled || (index === 0 ? "disabled" : "")}>↑</button>
        <button type="button" data-action="down" aria-label="Move ${escapeHtml(layer.name)} layer later" ${disabled || (index === ordered.length - 1 ? "disabled" : "")}>↓</button>
        <button type="button" data-action="lock" aria-label="${layer.locked ? "Unlock" : "Lock"} ${escapeHtml(layer.name)}" ${disabled}>${layer.locked ? "▣" : "□"}</button>`;
      elements.layerList.append(row);
    }
  }

  async function reorderSemanticLayer(layerId, direction) {
    const ordered = [...state.layers].sort((a, b) => a.position - b.position || a.id.localeCompare(b.id));
    const index = ordered.findIndex((layer) => layer.id === layerId);
    const target = direction === "up" ? index - 1 : index + 1;
    if (index < 0 || target < 0 || target >= ordered.length) return;
    const before = ordered.map((layer, position) => ({ id: layer.id, position: position + 1 }));
    [ordered[index], ordered[target]] = [ordered[target], ordered[index]];
    const after = ordered.map((layer, position) => ({ id: layer.id, position: position + 1 }));
    for (const item of after) Object.assign(layerFor(item.id) || {}, { position: item.position });
    renderAll();
    try {
      await executeCommand("layers.reorder", { before_json: before, after_json: after });
      await reloadDiagramModel();
      showToast("Semantic layer order saved in SQLite.");
    } catch (error) {
      for (const item of before) Object.assign(layerFor(item.id) || {}, { position: item.position });
      renderAll();
      showToast(error.message, true);
    }
  }

  async function updateLayerState(layerId, action) {
    const layer = layerFor(layerId);
    if (!layer || !["visibility", "lock"].includes(action)) return;
    const previous = { visible: layer.visible, locked: layer.locked };
    if (action === "visibility") layer.visible = layer.visible ? 0 : 1;
    if (action === "lock") layer.locked = layer.locked ? 0 : 1;
    renderAll();
    try {
      await executeCommand("layer.update", {
        layer_id: layer.id,
        from_visible: previous.visible,
        from_locked: previous.locked,
        to_visible: layer.visible,
        to_locked: layer.locked,
      });
      showToast(`${layer.name} layer ${action === "visibility" ? (layer.visible ? "shown" : "hidden") : (layer.locked ? "locked" : "unlocked")}.`);
    } catch (error) {
      Object.assign(layer, previous);
      renderAll();
      showToast(error.message, true);
    }
  }

  async function toggleSelectedGroup(existingGroup) {
    const nodes = state.nodes.filter((node) => state.selectedNodeIds.includes(node.id));
    if (!nodes.length) return;
    const action = existingGroup ? "ungroup" : "group";
    try {
      await executeCommand("group.toggle", {
        action,
        group_id: existingGroup?.id || uid("group"),
        layer_id: existingGroup?.layer_id || nodes[0].layer_id,
        name: existingGroup?.name || `Group ${state.groups.length + 1}`,
        node_ids_json: nodes.map((node) => node.id),
      });
      await reloadDiagramModel();
      showToast(`${action === "group" ? "Grouped" : "Ungrouped"} ${nodes.length} nodes.`);
    } catch (error) {
      showToast(error.message, true);
    }
  }

  function renderInspector() {
    if (state.selectedType === "node" && state.selectedNodeIds.length > 1) {
      const selectedGroup = state.groups.find((group) => {
        const members = Array.isArray(group.member_ids_json) ? group.member_ids_json : [];
        return members.length === state.selectedNodeIds.length && state.selectedNodeIds.every((id) => members.includes(id));
      });
      const selectedLayers = new Set(state.nodes.filter((node) => state.selectedNodeIds.includes(node.id)).map((node) => node.layer_id));
      elements.inspector.innerHTML = `
        <div class="inspector-card">
          <div class="selection-type"><div><span>Multi-selection</span><strong>${state.selectedNodeIds.length} nodes</strong></div></div>
          <p class="inspector-description">${canEdit() ? "Shift-click toggles nodes. Arrow keys move the selection; Shift+Arrow resizes it." : "Shift-click toggles nodes for read-only inspection and export."}</p>
          <div class="alignment-grid" role="group" aria-label="Align selected nodes">
            <button class="inspector-button" type="button" data-align="left">Left</button>
            <button class="inspector-button" type="button" data-align="center">Center</button>
            <button class="inspector-button" type="button" data-align="right">Right</button>
            <button class="inspector-button" type="button" data-align="top">Top</button>
            <button class="inspector-button" type="button" data-align="middle">Middle</button>
            <button class="inspector-button" type="button" data-align="bottom">Bottom</button>
            <button class="inspector-button" type="button" data-distribute="horizontal" ${state.selectedNodeIds.length < 3 ? "disabled" : ""}>Distribute H</button>
            <button class="inspector-button" type="button" data-distribute="vertical" ${state.selectedNodeIds.length < 3 ? "disabled" : ""}>Distribute V</button>
          </div>
          <div class="inspector-actions">
            <button id="toggle-group" class="inspector-button" type="button" ${!selectedGroup && selectedLayers.size !== 1 ? "disabled" : ""}>${selectedGroup ? "Ungroup" : "Group"}</button>
            <button id="delete-selection" class="inspector-button is-danger" type="button">Delete selection</button>
          </div>
          <div class="field-group"><label for="selection-layer">Move selection to layer</label><select id="selection-layer" class="inspector-input">${layerOptions(selectedLayers.size === 1 ? [...selectedLayers][0] : "")}</select></div>
          <div class="inspector-actions structure-actions">
            <button id="move-selection-layer" class="inspector-button" type="button">Move to layer</button>
            <button class="inspector-button" type="button" data-stack="front">Bring front</button>
            <button class="inspector-button" type="button" data-stack="back">Send back</button>
          </div>
        </div>`;
      elements.inspector.querySelectorAll("[data-align]").forEach((button) => button.addEventListener("click", () => {
        const nodes = selectedUnlockedNodes();
        applyNodeTransforms(geometry.alignChanges(nodes, button.dataset.align), `Align ${button.dataset.align}`);
      }));
      elements.inspector.querySelectorAll("[data-distribute]").forEach((button) => button.addEventListener("click", () => {
        const nodes = selectedUnlockedNodes();
        applyNodeTransforms(geometry.distributeChanges(nodes, button.dataset.distribute), `Distribute ${button.dataset.distribute}`);
      }));
      document.getElementById("toggle-group")?.addEventListener("click", () => toggleSelectedGroup(selectedGroup));
      document.getElementById("delete-selection")?.addEventListener("click", deleteSelection);
      document.getElementById("move-selection-layer")?.addEventListener("click", () => structureSelectedNodes("layer", document.getElementById("selection-layer")?.value));
      elements.inspector.querySelectorAll("[data-stack]").forEach((button) => button.addEventListener("click", () => structureSelectedNodes(button.dataset.stack)));
      return;
    }
    if (state.selectedType === "node") {
      const node = state.nodes.find((item) => item.id === state.selectedId);
      if (node) return renderNodeInspector(node);
    }
    if (state.selectedType === "edge") {
      const edge = state.edges.find((item) => item.id === state.selectedId);
      if (edge) return renderEdgeInspector(edge);
    }
    elements.inspector.innerHTML = `
      <div class="empty-inspector">
        <div class="empty-icon">◇</div>
        <h3>Select an object</h3>
        <p>${canEdit() ? "Inspect a node or connector. Drag unlocked nodes to persist their position in the database." : "Inspect a node or connector. This profile cannot persist changes to the database."}</p>
      </div>`;
  }

  function renderNodeInspector(node) {
    const style = nodeStyle(node);
    const data = dataFor(node);
    const locked = isNodeLocked(node);
    elements.inspector.innerHTML = `
      <div class="inspector-card" style="--selection-accent: ${style.accent}">
        <div class="selection-type">
          <span class="selection-swatch"></span>
          <div>
            <span>${escapeHtml(node.kind)}</span>
            <strong>${escapeHtml(node.label)}</strong>
          </div>
        </div>
        <div class="field-group">
          <label for="node-label-input">Label</label>
          <input id="node-label-input" class="inspector-input" value="${escapeHtml(node.label)}" ${locked ? "disabled" : ""}>
        </div>
        <div class="property-grid">
          <div class="property"><span>X</span><strong id="node-x-value">${Math.round(node.x)}</strong></div>
          <div class="property"><span>Y</span><strong id="node-y-value">${Math.round(node.y)}</strong></div>
          <label class="field-group"><span>Width</span><input id="node-width-input" class="inspector-input" type="number" min="60" max="4000" step="12" value="${Math.round(node.width)}" ${locked ? "disabled" : ""}></label>
          <label class="field-group"><span>Height</span><input id="node-height-input" class="inspector-input" type="number" min="40" max="4000" step="12" value="${Math.round(node.height)}" ${locked ? "disabled" : ""}></label>
        </div>
        <p class="inspector-description">${escapeHtml(data.description || "No description stored for this node.")}</p>
        <div class="field-group"><label for="node-layer-input">Semantic layer</label><select id="node-layer-input" class="inspector-input" ${locked ? "disabled" : ""}>${layerOptions(node.layer_id)}</select></div>
        <div class="inspector-actions">
          <button id="save-node-label" class="inspector-button" type="button" ${locked ? "disabled" : ""}>Save label</button>
          <button id="save-node-size" class="inspector-button" type="button" ${locked ? "disabled" : ""}>Save size</button>
          <button id="delete-selection" class="inspector-button is-danger" type="button" ${locked ? "disabled" : ""}>Delete</button>
        </div>
        <div class="inspector-actions structure-actions">
          <button id="move-node-layer" class="inspector-button" type="button" ${locked ? "disabled" : ""}>Move to layer</button>
          <button class="inspector-button" type="button" data-stack="front" ${locked ? "disabled" : ""}>Bring front</button>
          <button class="inspector-button" type="button" data-stack="back" ${locked ? "disabled" : ""}>Send back</button>
        </div>
      </div>`;
    const input = document.getElementById("node-label-input");
    document.getElementById("save-node-label")?.addEventListener("click", () => renameNode(node, input.value));
    document.getElementById("save-node-size")?.addEventListener("click", () => resizeNode(
      node,
      document.getElementById("node-width-input")?.value,
      document.getElementById("node-height-input")?.value,
    ));
    input?.addEventListener("keydown", (event) => {
      if (event.key === "Enter") renameNode(node, input.value);
    });
    document.getElementById("delete-selection")?.addEventListener("click", deleteSelection);
    document.getElementById("move-node-layer")?.addEventListener("click", () => structureSelectedNodes("layer", document.getElementById("node-layer-input")?.value));
    elements.inspector.querySelectorAll("[data-stack]").forEach((button) => button.addEventListener("click", () => structureSelectedNodes(button.dataset.stack)));
  }

  function renderEdgeInspector(edge) {
    const source = state.nodes.find((node) => node.id === edge.source_id);
    const target = state.nodes.find((node) => node.id === edge.target_id);
    const style = edgeStyle(edge);
    elements.inspector.innerHTML = `
      <div class="inspector-card" style="--selection-accent: ${style.stroke}">
        <div class="selection-type">
          <span class="selection-swatch"></span>
          <div>
            <span>${escapeHtml(edge.kind)} connector</span>
            <strong>${escapeHtml(edge.label || "Unlabelled relationship")}</strong>
          </div>
        </div>
        <div class="property-grid">
          <div class="property"><span>Source</span><strong>${escapeHtml(source?.label || edge.source_id)}</strong></div>
          <div class="property"><span>Target</span><strong>${escapeHtml(target?.label || edge.target_id)}</strong></div>
        </div>
        <div class="field-group"><label for="edge-source-node">Source node</label><select id="edge-source-node" class="inspector-input">${state.nodes.filter((node) => node.id !== edge.target_id).map((node) => `<option value="${escapeHtml(node.id)}" ${node.id === edge.source_id ? "selected" : ""}>${escapeHtml(node.label)}</option>`).join("")}</select></div>
        <div class="field-group"><label for="edge-target-node">Target node</label><select id="edge-target-node" class="inspector-input">${state.nodes.filter((node) => node.id !== edge.source_id).map((node) => `<option value="${escapeHtml(node.id)}" ${node.id === edge.target_id ? "selected" : ""}>${escapeHtml(node.label)}</option>`).join("")}</select></div>
        <div class="property-grid">
          <label class="field-group"><span>Source port</span><select id="edge-source-port" class="inspector-input">${["auto", "north", "east", "south", "west"].map((port) => `<option ${port === edge.source_port ? "selected" : ""}>${port}</option>`).join("")}</select></label>
          <label class="field-group"><span>Target port</span><select id="edge-target-port" class="inspector-input">${["auto", "north", "east", "south", "west"].map((port) => `<option ${port === edge.target_port ? "selected" : ""}>${port}</option>`).join("")}</select></label>
        </div>
        <div class="field-group"><label for="edge-route-mode">Routing</label><select id="edge-route-mode" class="inspector-input"><option value="orthogonal" ${edge.route_mode === "orthogonal" ? "selected" : ""}>Obstacle-aware orthogonal</option><option value="direct" ${edge.route_mode === "direct" ? "selected" : ""}>Direct curve</option></select></div>
        <p class="inspector-description">This relationship is a semantic row in <code>diagram_edge</code>, not a baked SVG path.</p>
        <div class="inspector-actions">
          <button id="save-edge-config" class="inspector-button" type="button">Save connector</button>
          <button id="delete-selection" class="inspector-button is-danger" type="button">Delete connector</button>
        </div>
      </div>`;
    document.getElementById("delete-selection")?.addEventListener("click", deleteSelection);
    document.getElementById("save-edge-config")?.addEventListener("click", () => configureEdge(edge, {
      source_id: document.getElementById("edge-source-node")?.value,
      target_id: document.getElementById("edge-target-node")?.value,
      source_port: document.getElementById("edge-source-port")?.value,
      target_port: document.getElementById("edge-target-port")?.value,
      route_mode: document.getElementById("edge-route-mode")?.value,
    }));
  }

  function selectObject(type, id, toggle = false) {
    if (type === "node") {
      if (toggle) {
        state.selectedNodeIds = state.selectedNodeIds.includes(id)
          ? state.selectedNodeIds.filter((item) => item !== id)
          : [...state.selectedNodeIds, id];
      } else {
        state.selectedNodeIds = [id];
      }
      state.selectedType = state.selectedNodeIds.length ? "node" : null;
      state.selectedId = state.selectedNodeIds.at(-1) || null;
    } else {
      state.selectedNodeIds = [];
      state.selectedType = type;
      state.selectedId = id;
    }
    renderDiagram();
    renderInspector();
  }

  function clearSelection() {
    state.selectedType = null;
    state.selectedId = null;
    state.selectedNodeIds = [];
    renderDiagram();
    renderInspector();
  }

  function activateNode(node, event = {}) {
    if (state.mode === "connect") {
      if (!state.connectSourceId) {
        state.connectSourceId = node.id;
        showToast(`Source selected: ${node.label}. Choose a target node.`);
        updateModeUI();
        renderDiagram();
        return;
      }
      if (state.connectSourceId === node.id) {
        state.connectSourceId = null;
        showToast("Connector source cleared.");
        updateModeUI();
        renderDiagram();
        return;
      }
      createEdge(state.connectSourceId, node.id);
      return;
    }
    selectObject("node", node.id, Boolean(event.shiftKey));
  }

  function startNodePointer(event, node) {
    if (state.mode === "connect") {
      event.stopPropagation();
      return;
    }
    if (!canEdit()) {
      event.stopPropagation();
      return;
    }
    if (isNodeLocked(node)) return;
    event.stopPropagation();
    if (event.shiftKey) return;
    cancelViewAnimation();
    if (!state.selectedNodeIds.includes(node.id)) selectObject("node", node.id);
    const matrix = elements.canvas.getScreenCTM()?.inverse() || null;
    const point = clientToWorld(event.clientX, event.clientY, matrix);
    const selected = state.nodes.filter((item) => state.selectedNodeIds.includes(item.id) && !isNodeLocked(item));
    state.drag = {
      pointerId: event.pointerId,
      nodeId: node.id,
      startPoint: point,
      matrix,
      starts: selected.map((item) => ({ id: item.id, ...geometry.frame(item) })),
      pending: { dx: 0, dy: 0 },
      moved: false,
    };
    elements.canvas.classList.add("is-gesturing");
    elements.canvas.setPointerCapture(event.pointerId);
  }

  function startResizePointer(event, node) {
    if (!canEdit()) return;
    event.preventDefault();
    event.stopPropagation();
    const matrix = elements.canvas.getScreenCTM()?.inverse() || null;
    const point = clientToWorld(event.clientX, event.clientY, matrix);
    state.resize = {
      pointerId: event.pointerId,
      nodeId: node.id,
      startPoint: point,
      matrix,
      startWidth: node.width,
      startHeight: node.height,
      pending: { width: node.width, height: node.height },
      moved: false,
    };
    elements.canvas.classList.add("is-gesturing");
    elements.canvas.setPointerCapture(event.pointerId);
  }

  function onCanvasPointerDown(event) {
    if (event.button !== 0) return;
    if (event.target.closest?.(".node") || event.target.closest?.(".edge")) return;
    cancelViewAnimation();
    if (event.shiftKey) {
      const point = clientToWorld(event.clientX, event.clientY, state.resize.matrix);
      state.marquee = { pointerId: event.pointerId, start: point, end: point };
      renderMarquee();
      elements.canvas.setPointerCapture(event.pointerId);
      return;
    }
    state.pan = {
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      viewBox: { ...state.viewBox },
      moved: false,
    };
    elements.canvas.setPointerCapture(event.pointerId);
  }

  function onCanvasPointerMove(event) {
    if (state.marquee && state.marquee.pointerId === event.pointerId) {
      state.marquee.end = clientToWorld(event.clientX, event.clientY);
      renderMarquee();
      return;
    }
    if (state.resize && state.resize.pointerId === event.pointerId) {
      const point = clientToWorld(event.clientX, event.clientY, state.resize.matrix);
      const step = event.shiftKey ? 1 : 12;
      state.resize.pending = {
        width: Math.max(geometry.MIN_WIDTH, geometry.snap(state.resize.startWidth + point.x - state.resize.startPoint.x, step)),
        height: Math.max(geometry.MIN_HEIGHT, geometry.snap(state.resize.startHeight + point.y - state.resize.startPoint.y, step)),
      };
      state.resize.moved = state.resize.pending.width !== state.resize.startWidth || state.resize.pending.height !== state.resize.startHeight;
      scheduleGesturePreview();
      return;
    }
    if (state.drag && state.drag.pointerId === event.pointerId) {
      const point = clientToWorld(event.clientX, event.clientY, state.drag.matrix);
      const step = event.shiftKey ? 1 : 12;
      const dx = geometry.snap(point.x - state.drag.startPoint.x, step);
      const dy = geometry.snap(point.y - state.drag.startPoint.y, step);
      state.drag.moved = dx !== 0 || dy !== 0;
      state.drag.pending = { dx, dy };
      scheduleGesturePreview();
      return;
    }
    if (state.pan && state.pan.pointerId === event.pointerId) {
      const rect = elements.canvas.getBoundingClientRect();
      const scaleX = state.pan.viewBox.width / rect.width;
      const scaleY = state.pan.viewBox.height / rect.height;
      const dx = (event.clientX - state.pan.clientX) * scaleX;
      const dy = (event.clientY - state.pan.clientY) * scaleY;
      if (Math.abs(dx) > 2 || Math.abs(dy) > 2) state.pan.moved = true;
      applyViewBox({
        ...state.pan.viewBox,
        x: state.pan.viewBox.x - dx,
        y: state.pan.viewBox.y - dy,
      });
    }
  }

  async function onCanvasPointerUp(event) {
    if (state.marquee && state.marquee.pointerId === event.pointerId) {
      const marquee = state.marquee;
      state.marquee = null;
      elements.marqueeLayer.replaceChildren();
      if (elements.canvas.hasPointerCapture(event.pointerId)) elements.canvas.releasePointerCapture(event.pointerId);
      const left = Math.min(marquee.start.x, marquee.end.x);
      const top = Math.min(marquee.start.y, marquee.end.y);
      const right = Math.max(marquee.start.x, marquee.end.x);
      const bottom = Math.max(marquee.start.y, marquee.end.y);
      state.selectedNodeIds = state.nodes
        .filter((node) => layerVisible(node.layer_id) && node.x < right && node.x + node.width > left && node.y < bottom && node.y + node.height > top)
        .sort((a, b) => layerPosition(a.layer_id) - layerPosition(b.layer_id) || a.z_index - b.z_index || a.id.localeCompare(b.id))
        .map((node) => node.id);
      state.selectedType = state.selectedNodeIds.length ? "node" : null;
      state.selectedId = state.selectedNodeIds.at(-1) || null;
      renderAll();
      return;
    }
    if (state.resize && state.resize.pointerId === event.pointerId) {
      if (state.resize.moved) flushGesturePreview();
      else cancelGesturePreview();
      const resize = state.resize;
      state.resize = null;
      elements.canvas.classList.remove("is-gesturing");
      if (elements.canvas.hasPointerCapture(event.pointerId)) elements.canvas.releasePointerCapture(event.pointerId);
      const node = state.nodes.find((item) => item.id === resize.nodeId);
      if (resize.moved && node) {
        renderDiagram();
        renderInspector();
        state.suppressClickUntil = Date.now() + 180;
        try {
          await executeCommand("node.resize", {
            id: node.id,
            from_width: resize.startWidth,
            from_height: resize.startHeight,
            to_width: node.width,
            to_height: node.height,
          });
          showToast(`Saved reversible size for “${node.label}”.`);
        } catch (error) {
          node.width = resize.startWidth;
          node.height = resize.startHeight;
          renderAll();
          showToast(error.message, true);
        }
      }
      return;
    }
    if (state.drag && state.drag.pointerId === event.pointerId) {
      if (state.drag.moved) flushGesturePreview();
      else cancelGesturePreview();
      const drag = state.drag;
      state.drag = null;
      elements.canvas.classList.remove("is-gesturing");
      if (elements.canvas.hasPointerCapture(event.pointerId)) elements.canvas.releasePointerCapture(event.pointerId);
      if (drag.moved) {
        renderDiagram();
        renderInspector();
        state.suppressClickUntil = Date.now() + 180;
        const changes = drag.starts.map((start) => {
          const node = state.nodes.find((item) => item.id === start.id);
          return geometry.transformChange(start, node || start);
        });
        try {
          await executeCommand("nodes.transform", {
            summary: changes.length > 1 ? `Move ${changes.length} nodes` : "Move node",
            changes_json: changes,
          });
          showToast(`Saved reversible position for ${changes.length > 1 ? `${changes.length} nodes` : "node"}.`);
        } catch (error) {
          for (const start of drag.starts) Object.assign(state.nodes.find((item) => item.id === start.id) || {}, start);
          renderAll();
          showToast(error.message, true);
        }
      }
      return;
    }
    if (state.pan && state.pan.pointerId === event.pointerId) {
      const moved = state.pan.moved;
      state.pan = null;
      if (elements.canvas.hasPointerCapture(event.pointerId)) elements.canvas.releasePointerCapture(event.pointerId);
      if (moved) state.suppressClickUntil = Date.now() + 120;
    }
  }

  function onCanvasClick(event) {
    if (Date.now() < state.suppressClickUntil) return;
    if (event.target === elements.background || event.target === elements.canvas) {
      if (state.mode === "connect" && state.connectSourceId) {
        state.connectSourceId = null;
        updateModeUI();
        renderDiagram();
      } else {
        clearSelection();
      }
    }
  }

  function renderMarquee() {
    elements.marqueeLayer.replaceChildren();
    if (!state.marquee) return;
    const left = Math.min(state.marquee.start.x, state.marquee.end.x);
    const top = Math.min(state.marquee.start.y, state.marquee.end.y);
    elements.marqueeLayer.append(svgElement("rect", {
      class: "selection-marquee",
      x: left,
      y: top,
      width: Math.abs(state.marquee.end.x - state.marquee.start.x),
      height: Math.abs(state.marquee.end.y - state.marquee.start.y),
    }));
  }

  function onCanvasWheel(event) {
    event.preventDefault();
    cancelViewAnimation();
    const pointer = clientToWorld(event.clientX, event.clientY);
    const factor = Math.exp(event.deltaY * 0.0013);
    const aspect = state.viewBox.height / state.viewBox.width;
    const nextWidth = clamp(state.viewBox.width * factor, MIN_VIEW_WIDTH, MAX_VIEW_WIDTH);
    const nextHeight = nextWidth * aspect;
    const ratioX = (pointer.x - state.viewBox.x) / state.viewBox.width;
    const ratioY = (pointer.y - state.viewBox.y) / state.viewBox.height;
    applyViewBox({
      x: pointer.x - nextWidth * ratioX,
      y: pointer.y - nextHeight * ratioY,
      width: nextWidth,
      height: nextHeight,
    });
  }

  function updatePointerCoordinates(event) {
    state.pointerClient = { x: event.clientX, y: event.clientY };
    if (state.pointerCoordinateFrame !== null) return;
    state.pointerCoordinateFrame = requestAnimationFrame(() => {
      state.pointerCoordinateFrame = null;
      if (!state.pointerClient) return;
      const point = clientToWorld(state.pointerClient.x, state.pointerClient.y);
      const zoom = state.homeView ? Math.round((state.homeView.width / state.viewBox.width) * 100) : 100;
      elements.coordinates.textContent = `cursor ${Math.round(point.x)}, ${Math.round(point.y)} · ${zoom}%`;
    });
  }

  function updateCoordinatesLabel() {
    const zoom = state.homeView ? Math.round((state.homeView.width / state.viewBox.width) * 100) : 100;
    elements.coordinates.textContent = `view ${Math.round(state.viewBox.x)}, ${Math.round(state.viewBox.y)} · ${zoom}%`;
  }

  function clientToWorld(clientX, clientY, inverseMatrix = null) {
    const matrix = inverseMatrix || elements.canvas.getScreenCTM()?.inverse();
    if (!matrix) return { x: 0, y: 0 };
    return {
      x: matrix.a * clientX + matrix.c * clientY + matrix.e,
      y: matrix.b * clientX + matrix.d * clientY + matrix.f,
    };
  }

  function applyViewBox(viewBox) {
    state.viewBox = {
      x: toFiniteNumber(viewBox.x),
      y: toFiniteNumber(viewBox.y),
      width: clamp(toFiniteNumber(viewBox.width, 2200), MIN_VIEW_WIDTH, MAX_VIEW_WIDTH),
      height: clamp(toFiniteNumber(viewBox.height, 1250), MIN_VIEW_WIDTH * 0.35, MAX_VIEW_WIDTH),
    };
    elements.canvas.setAttribute(
      "viewBox",
      `${state.viewBox.x} ${state.viewBox.y} ${state.viewBox.width} ${state.viewBox.height}`,
    );
    updateCoordinatesLabel();
  }

  function calculateBounds(padding = 80) {
    if (!state.nodes.length) return { x: 0, y: 0, width: 2200, height: 1250 };
    const minimumX = Math.min(...state.nodes.map((node) => node.x));
    const minimumY = Math.min(...state.nodes.map((node) => node.y));
    const maximumX = Math.max(...state.nodes.map((node) => node.x + node.width));
    const maximumY = Math.max(...state.nodes.map((node) => node.y + node.height));
    const width = Math.max(400, maximumX - minimumX + padding * 2);
    const height = Math.max(260, maximumY - minimumY + padding * 2);
    return { x: minimumX - padding, y: minimumY - padding, width, height };
  }

  function animateViewBox(target, duration = 650) {
    cancelViewAnimation();
    const start = { ...state.viewBox };
    const started = performance.now();
    const step = (now) => {
      const elapsed = clamp((now - started) / duration, 0, 1);
      const eased = elapsed < 0.5 ? 4 * elapsed ** 3 : 1 - Math.pow(-2 * elapsed + 2, 3) / 2;
      applyViewBox({
        x: start.x + (target.x - start.x) * eased,
        y: start.y + (target.y - start.y) * eased,
        width: start.width + (target.width - start.width) * eased,
        height: start.height + (target.height - start.height) * eased,
      });
      if (elapsed < 1) state.animationFrame = requestAnimationFrame(step);
      else state.animationFrame = null;
    };
    state.animationFrame = requestAnimationFrame(step);
  }

  function cancelViewAnimation() {
    if (state.animationFrame !== null) {
      cancelAnimationFrame(state.animationFrame);
      state.animationFrame = null;
    }
  }

  function applyScene(index) {
    if (index < 0 || index >= state.scenes.length) return;
    const scene = state.scenes[index];
    const previousFrames = new Map(state.nodes.map((node) => [node.id, geometry.frame(effectiveNode(node))]));
    state.sceneIndex = index;
    state.focusIds = new Set(Array.isArray(scene.focus_json) ? scene.focus_json : []);
    state.sceneOverrides = new Map((Array.isArray(scene.overrides_json) ? scene.overrides_json : []).map((override) => [override.node_id, override]));
    const viewport = scene.viewport_json || state.homeView;
    animateViewBox({
      x: toFiniteNumber(viewport.x),
      y: toFiniteNumber(viewport.y),
      width: toFiniteNumber(viewport.width, state.homeView.width),
      height: toFiniteNumber(viewport.height, state.homeView.height),
    });
    renderSceneList();
    renderDiagram();
    animateSceneNodes(previousFrames);
    updatePresentationOverlay();
  }

  function clearSceneFocus() {
    state.sceneIndex = -1;
    state.focusIds = null;
    state.sceneOverrides = new Map();
    renderSceneList();
    renderDiagram();
  }

  function normalisedScenes(scenes) {
    return scenes.map((scene, index) => ({
      id: scene.id,
      diagram_id: DIAGRAM_ID,
      position: index + 1,
      title: String(scene.title || `Scene ${index + 1}`),
      narrative: String(scene.narrative || ""),
      viewport_json: scene.viewport_json || { ...state.viewBox },
      focus_json: Array.isArray(scene.focus_json) ? scene.focus_json : [],
      overrides_json: Array.isArray(scene.overrides_json) ? scene.overrides_json : [],
    }));
  }

  async function mutateScenes(action) {
    const before = normalisedScenes(state.scenes);
    const after = before.map((scene) => structuredClone(scene));
    let index = state.sceneIndex < 0 ? 0 : state.sceneIndex;
    let selectedId = after[index]?.id;
    if (action === "add") {
      const title = window.prompt("Scene title", `Scene ${after.length + 1}`);
      if (!title) return;
      const scene = {
        id: uid("scene"), diagram_id: DIAGRAM_ID, position: after.length + 1, title,
        narrative: "Authored in Diagram Studio.", viewport_json: { ...state.viewBox },
        focus_json: [...state.selectedNodeIds], overrides_json: [],
      };
      after.push(scene);
      selectedId = scene.id;
    } else if (action === "rename") {
      if (!after[index]) return;
      const title = window.prompt("Rename scene", after[index].title);
      if (!title || title === after[index].title) return;
      after[index].title = title;
    } else if (action === "capture") {
      if (!after[index]) return;
      after[index].viewport_json = { ...state.viewBox };
      after[index].focus_json = [...state.selectedNodeIds];
      after[index].overrides_json = state.nodes
        .filter((node) => state.selectedNodeIds.includes(node.id))
        .map((node) => ({ node_id: node.id, x: node.x, y: node.y, width: node.width, height: node.height, visible: 1, style_json: null }));
    } else if (action === "duplicate") {
      if (!after[index]) return;
      const copy = structuredClone(after[index]);
      copy.id = uid("scene");
      copy.title = `${copy.title} copy`;
      after.splice(index + 1, 0, copy);
      selectedId = copy.id;
    } else if (action === "delete") {
      if (after.length <= 1 || !after[index]) return;
      if (!window.confirm(`Delete scene “${after[index].title}”?`)) return;
      after.splice(index, 1);
      selectedId = after[Math.min(index, after.length - 1)].id;
    } else if (action === "up" || action === "down") {
      const next = index + (action === "up" ? -1 : 1);
      if (next < 0 || next >= after.length) return;
      [after[index], after[next]] = [after[next], after[index]];
    } else return;
    try {
      await executeCommand("scenes.apply", {
        summary: `${action[0].toUpperCase()}${action.slice(1)} scene`,
        before_json: before,
        after_json: normalisedScenes(after),
      });
      state.scenes = await readEndpoint("diagram.scenes", { diagram_id: DIAGRAM_ID });
      index = Math.max(0, state.scenes.findIndex((scene) => scene.id === selectedId));
      applyScene(index);
      showToast(`Scene ${action} saved in SQLite.`);
    } catch (error) {
      showToast(error.message, true);
    }
  }

  function effectiveNode(node) {
    const override = state.sceneOverrides.get(node.id);
    if (!override) return node;
    if (override.visible === 0) return { ...node, _sceneHidden: true };
    return {
      ...node,
      x: override.x ?? node.x,
      y: override.y ?? node.y,
      width: override.width ?? node.width,
      height: override.height ?? node.height,
      style_json: override.style_json || node.style_json,
    };
  }

  function animateSceneNodes(previousFrames) {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    for (const node of state.nodes) {
      const element = elements.elementLayer.querySelector(`.node[data-id="${CSS.escape(node.id)}"]`);
      const before = previousFrames.get(node.id);
      const after = geometry.frame(effectiveNode(node));
      if (!element || !before || (before.x === after.x && before.y === after.y && before.width === after.width && before.height === after.height)) continue;
      element.animate(
        [
          { transform: `translate(${before.x}px, ${before.y}px)`, opacity: 0.72 },
          { transform: `translate(${after.x}px, ${after.y}px)`, opacity: 1 },
        ],
        { duration: 520, easing: "cubic-bezier(.2,.8,.2,1)" },
      );
    }
  }

  function moveScene(delta) {
    if (!state.scenes.length) return;
    const current = state.sceneIndex < 0 ? 0 : state.sceneIndex;
    const next = clamp(current + delta, 0, state.scenes.length - 1);
    applyScene(next);
  }

  async function enterPresentation() {
    state.presenting = true;
    document.body.classList.add("presenting");
    if (state.sceneIndex < 0) applyScene(0);
    else updatePresentationOverlay();
    try {
      await document.documentElement.requestFullscreen?.();
    } catch {
      // Fullscreen is optional; presentation mode still works in the tab.
    }
  }

  function exitPresentation(exitFullscreen = true) {
    state.presenting = false;
    document.body.classList.remove("presenting");
    if (exitFullscreen && document.fullscreenElement) {
      document.exitFullscreen?.().catch(() => {});
    }
  }

  function updatePresentationOverlay() {
    if (state.sceneIndex < 0 || !state.scenes[state.sceneIndex]) return;
    const scene = state.scenes[state.sceneIndex];
    elements.presentationIndex.textContent = `${state.sceneIndex + 1} / ${state.scenes.length}`;
    elements.presentationTitle.textContent = scene.title;
    elements.presentationNarrative.textContent = scene.narrative;
    elements.previousScene.disabled = state.sceneIndex === 0;
    elements.nextScene.disabled = state.sceneIndex === state.scenes.length - 1;
  }

  function toggleConnectMode() {
    state.mode = state.mode === "connect" ? "select" : "connect";
    state.connectSourceId = null;
    updateModeUI();
    renderDiagram();
    if (state.mode === "connect") showToast("Connector mode: choose a source node, then a target.");
  }

  function updateModeUI() {
    const connecting = state.mode === "connect";
    elements.connectMode.classList.toggle("is-active", connecting);
    elements.canvas.classList.toggle("is-connecting", connecting);
    elements.modeHint.hidden = !connecting;
    elements.modeHint.textContent = state.connectSourceId
      ? "Connector mode · choose a target node · Esc to cancel"
      : "Connector mode · choose a source node · Esc to cancel";
  }

  async function addNode() {
    clearSceneFocus();
    const shape = elements.shapePicker.value || "rounded-rectangle";
    const width = shape === "container" ? 420 : 240;
    const height = shape === "container" ? 220 : shape === "pill" ? 92 : 125;
    const centerX = state.viewBox.x + state.viewBox.width / 2;
    const centerY = state.viewBox.y + state.viewBox.height / 2;
    const node = {
      id: uid("node"),
      diagram_id: DIAGRAM_ID,
      layer_id: "layer-content",
      kind: shape === "container" ? "container" : "note",
      label: shape === "container" ? "New container" : "New capsule concept",
      x: Math.round((centerX - width / 2) / 12) * 12,
      y: Math.round((centerY - height / 2) / 12) * 12,
      width,
      height,
      z_index: Math.max(0, ...state.nodes.map((item) => item.z_index)) + 1,
      style_json: {
        fill: "#172554",
        stroke: "#60a5fa",
        text: "#eff6ff",
        accent: "#93c5fd",
        radius: 18,
      },
      data_json: {
        eyebrow: "NEW NODE",
        shape,
        description: "Created in the browser and persisted through the capsule's named API.",
      },
    };
    try {
      const { layer_id: _defaultLayer, ...parameters } = node;
      await executeCommand("node.create", parameters);
      state.nodes.push(node);
      selectObject("node", node.id);
      showToast("New node inserted into diagram_node.");
      setTimeout(() => document.getElementById("node-label-input")?.select(), 0);
    } catch (error) {
      showToast(error.message, true);
    }
  }

  async function createEdge(sourceId, targetId) {
    const edge = {
      id: uid("edge"),
      diagram_id: DIAGRAM_ID,
      source_id: sourceId,
      target_id: targetId,
      kind: "flow",
      label: "",
      layer_id: "layer-connectors",
      source_port: "auto",
      target_port: "auto",
      route_mode: "orthogonal",
      waypoints_json: [],
      style_json: { stroke: "#7dd3fc", width: 2.5, curve: 0 },
    };
    try {
      await executeCommand("edge.create", {
        id: edge.id,
        diagram_id: edge.diagram_id,
        source_id: edge.source_id,
        target_id: edge.target_id,
        kind: edge.kind,
        label: edge.label,
        style_json: edge.style_json,
      });
      state.edges.push(edge);
      state.mode = "select";
      state.connectSourceId = null;
      updateModeUI();
      selectObject("edge", edge.id);
      showToast("Connector inserted into diagram_edge.");
    } catch (error) {
      showToast(error.message, true);
    }
  }

  async function configureEdge(edge, next) {
    if (!next.source_id || !next.target_id || next.source_id === next.target_id) {
      showToast("A connector needs two different nodes.", true);
      return;
    }
    const previous = {
      source_id: edge.source_id,
      target_id: edge.target_id,
      source_port: edge.source_port || "auto",
      target_port: edge.target_port || "auto",
      route_mode: edge.route_mode || "orthogonal",
    };
    if (Object.keys(previous).every((key) => previous[key] === next[key])) return;
    Object.assign(edge, next);
    renderAll();
    try {
      await executeCommand("edge.configure", {
        id: edge.id,
        from_source_id: previous.source_id,
        from_target_id: previous.target_id,
        from_source_port: previous.source_port,
        from_target_port: previous.target_port,
        from_route_mode: previous.route_mode,
        to_source_id: next.source_id,
        to_target_id: next.target_id,
        to_source_port: next.source_port,
        to_target_port: next.target_port,
        to_route_mode: next.route_mode,
      });
      showToast("Connector endpoints and route saved.");
    } catch (error) {
      Object.assign(edge, previous);
      renderAll();
      showToast(error.message, true);
    }
  }

  async function renameNode(node, rawLabel) {
    const label = String(rawLabel ?? "").trim();
    if (!label || label === node.label) {
      renderInspector();
      return;
    }
    const previous = node.label;
    node.label = label;
    renderDiagram();
    renderInspector();
    try {
      await executeCommand("node.rename", { id: node.id, from_label: previous, to_label: label });
      showToast("Node label saved in SQLite.");
    } catch (error) {
      node.label = previous;
      renderDiagram();
      renderInspector();
      showToast(error.message, true);
    }
  }

  async function resizeNode(node, rawWidth, rawHeight) {
    const width = Math.max(geometry.MIN_WIDTH, Math.min(4000, Number(rawWidth)));
    const height = Math.max(geometry.MIN_HEIGHT, Math.min(4000, Number(rawHeight)));
    if (!Number.isFinite(width) || !Number.isFinite(height) || (width === node.width && height === node.height)) {
      renderInspector();
      return;
    }
    const previous = { width: node.width, height: node.height };
    node.width = width;
    node.height = height;
    renderDiagram();
    renderInspector();
    try {
      await executeCommand("node.resize", {
        id: node.id,
        from_width: previous.width,
        from_height: previous.height,
        to_width: width,
        to_height: height,
      });
      showToast("Node size saved in SQLite.");
    } catch (error) {
      Object.assign(node, previous);
      renderAll();
      showToast(error.message, true);
    }
  }

  function selectedUnlockedNodes() {
    return state.nodes.filter((node) => state.selectedNodeIds.includes(node.id) && !isNodeLocked(node));
  }

  function layerOptions(selectedLayerId) {
    return [...state.layers]
      .sort((a, b) => a.position - b.position || a.id.localeCompare(b.id))
      .map((layer) => `<option value="${escapeHtml(layer.id)}" ${layer.id === selectedLayerId ? "selected" : ""} ${layer.locked ? "disabled" : ""}>${escapeHtml(layer.name)}${layer.locked ? " (locked)" : ""}</option>`)
      .join("");
  }

  async function structureSelectedNodes(action, targetLayerId = null) {
    const selected = selectedUnlockedNodes();
    if (!selected.length) return;
    let changes = [];
    if (action === "layer") {
      const target = layerFor(targetLayerId);
      if (!target || target.locked) {
        showToast("Choose an unlocked semantic layer.", true);
        return;
      }
      const groupedIds = new Set(state.groups.flatMap((group) => Array.isArray(group.member_ids_json) ? group.member_ids_json : []));
      if (selected.some((node) => groupedIds.has(node.id) && node.layer_id !== target.id)) {
        showToast("Ungroup nodes before moving them to another layer.", true);
        return;
      }
      const top = Math.max(-1, ...state.nodes.filter((node) => node.layer_id === target.id).map((node) => node.z_index));
      changes = selected.map((node, index) => ({
        id: node.id,
        from: { layer_id: node.layer_id, z_index: node.z_index },
        to: { layer_id: target.id, z_index: top + index + 1 },
      })).filter((change) => change.from.layer_id !== change.to.layer_id || change.from.z_index !== change.to.z_index);
    } else {
      for (const layerId of new Set(selected.map((node) => node.layer_id))) {
        const inLayer = selected
          .filter((node) => node.layer_id === layerId)
          .sort((a, b) => a.z_index - b.z_index || a.id.localeCompare(b.id));
        const allZ = state.nodes.filter((node) => node.layer_id === layerId).map((node) => node.z_index);
        const edge = action === "front" ? Math.max(-1, ...allZ) : Math.min(0, ...allZ) - inLayer.length;
        inLayer.forEach((node, index) => changes.push({
          id: node.id,
          from: { layer_id: node.layer_id, z_index: node.z_index },
          to: { layer_id: node.layer_id, z_index: edge + index + (action === "front" ? 1 : 0) },
        }));
      }
    }
    if (!changes.length) return;
    for (const change of changes) Object.assign(state.nodes.find((node) => node.id === change.id) || {}, change.to);
    renderAll();
    const summary = action === "layer" ? `Move ${changes.length} node${changes.length === 1 ? "" : "s"} to layer` : action === "front" ? "Bring selection to front" : "Send selection to back";
    try {
      await executeCommand("nodes.structure", { summary, changes_json: changes });
      await reloadDiagramModel();
      showToast(`${summary} saved in SQLite.`);
    } catch (error) {
      for (const change of changes) Object.assign(state.nodes.find((node) => node.id === change.id) || {}, change.from);
      renderAll();
      showToast(error.message, true);
    }
  }

  async function applyNodeTransforms(changes, summary) {
    if (!changes.length) return;
    for (const change of changes) Object.assign(state.nodes.find((node) => node.id === change.id) || {}, change.to);
    renderAll();
    try {
      await executeCommand("nodes.transform", { summary, changes_json: changes });
      showToast(`${summary} saved in SQLite.`);
    } catch (error) {
      for (const change of changes) Object.assign(state.nodes.find((node) => node.id === change.id) || {}, change.from);
      renderAll();
      showToast(error.message, true);
    }
  }

  function previewLayout() {
    const nodes = selectedUnlockedNodes().length ? selectedUnlockedNodes() : state.nodes.filter((node) => !isNodeLocked(node));
    if (!nodes.length) return;
    const mode = elements.layoutPicker.value;
    const changes = mode === "directional"
      ? geometry.directionalLayout(nodes)
      : mode === "layered"
        ? geometry.layeredLayout(nodes, state.edges)
        : geometry.gridLayout(nodes);
    for (const change of changes) Object.assign(state.nodes.find((node) => node.id === change.id) || {}, change.to);
    renderAll();
    if (window.confirm(`Apply the ${mode} layout preview as one undoable operation?`)) {
      applyNodeTransforms(changes, `Apply ${mode} layout`);
    } else {
      for (const change of changes) Object.assign(state.nodes.find((node) => node.id === change.id) || {}, change.from);
      renderAll();
      showToast("Layout preview cancelled.");
    }
  }

  function interchangeDocument(selectionOnly = false) {
    const selectedIds = new Set(selectionOnly ? state.selectedNodeIds : state.nodes.map((node) => node.id));
    if (selectionOnly && !selectedIds.size) throw new Error("Select one or more nodes first.");
    const nodes = state.nodes
      .filter((node) => selectedIds.has(node.id))
      .sort((a, b) => layerPosition(a.layer_id) - layerPosition(b.layer_id) || a.z_index - b.z_index || a.id.localeCompare(b.id))
      .map((node) => structuredClone(node));
    const edges = state.edges
      .filter((edge) => selectedIds.has(edge.source_id) && selectedIds.has(edge.target_id))
      .sort((a, b) => layerPosition(a.layer_id) - layerPosition(b.layer_id) || a.id.localeCompare(b.id))
      .map((edge) => structuredClone(edge));
    const layerIds = new Set([...nodes.map((node) => node.layer_id), ...edges.map((edge) => edge.layer_id)]);
    const layers = state.layers
      .filter((layer) => layerIds.has(layer.id))
      .sort((a, b) => a.position - b.position || a.id.localeCompare(b.id))
      .map((layer) => ({ id: layer.id, name: layer.name, position: layer.position, visible: layer.visible, locked: layer.locked }));
    const groups = state.groups
      .filter((group) => (group.member_ids_json || []).every((id) => selectedIds.has(id)))
      .map((group) => ({
        id: group.id, layer_id: group.layer_id, name: group.name, z_index: group.z_index,
        locked: group.locked, member_ids: [...group.member_ids_json],
      }));
    const scenes = selectionOnly ? [] : state.scenes.map((scene) => ({
      id: scene.id, title: scene.title, narrative: scene.narrative, position: scene.position,
      viewport_json: scene.viewport_json, focus_json: scene.focus_json, overrides_json: scene.overrides_json || [],
    }));
    return interchange.validate({
      format: interchange.FORMAT,
      title: state.diagram.title,
      description: state.diagram.description,
      layers, nodes, edges, groups, scenes,
    });
  }

  async function copySelection() {
    try {
      const text = JSON.stringify(interchangeDocument(true), null, 2);
      try {
        await navigator.clipboard.writeText(text);
        showToast("Selection copied as versioned Diagram Studio JSON.");
      } catch {
        window.prompt("Clipboard access was denied. Copy this Diagram Studio JSON:", text);
        showToast("Clipboard denied; explicit copy fallback opened.");
      }
    } catch (error) {
      showToast(error.message, true);
    }
  }

  async function pasteSelection() {
    let text;
    try {
      text = await navigator.clipboard.readText();
    } catch {
      text = window.prompt("Clipboard access was denied. Paste Diagram Studio JSON here:");
    }
    if (!text) return;
    try {
      await importInterchange(JSON.parse(text), "paste");
    } catch (error) {
      showToast(error.message, true);
    }
  }

  async function importSelectedFile(event) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > 1024 * 1024) {
      showToast("Import file exceeds the 1 MiB browser limit.", true);
      return;
    }
    try {
      await importInterchange(JSON.parse(await file.text()), "import");
    } catch (error) {
      showToast(error.message, true);
    }
  }

  async function importInterchange(raw, source) {
    const validated = interchange.validate(raw);
    const remapped = interchange.remap(validated, uid(source).slice(-12));
    if (!window.confirm(`Import ${remapped.nodes.length} nodes, ${remapped.edges.length} edges, ${remapped.groups.length} groups, and ${remapped.scenes.length} scenes with collision-safe remapped IDs?`)) return;
    await executeCommand("diagram.import", { document_json: remapped });
    state.scenes = await readEndpoint("diagram.scenes", { diagram_id: DIAGRAM_ID });
    state.selectedNodeIds = remapped.nodes.map((node) => node.id);
    state.selectedType = state.selectedNodeIds.length ? "node" : null;
    state.selectedId = state.selectedNodeIds.at(-1) || null;
    await reloadDiagramModel();
    showToast(`Imported ${remapped.nodes.length} nodes atomically.`);
  }

  function download(name, type, content) {
    const url = URL.createObjectURL(new Blob([content], { type }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = name;
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  async function exportDiagram() {
    try {
      const document = interchangeDocument(false);
      const format = elements.exportPicker.value;
      if (format === "json") {
        download("diagram-studio.json", "application/json", JSON.stringify(document, null, 2));
      } else {
        const svg = interchange.toSvg(document, geometry);
        if (format === "svg") {
          download("diagram-studio.svg", "image/svg+xml", svg);
        } else {
          await exportPng(svg);
        }
      }
      showToast(`Offline ${format.toUpperCase()} export prepared.`);
    } catch (error) {
      showToast(error.message, true);
    }
  }

  async function exportPng(svg) {
    const image = new Image();
    image.decoding = "async";
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    const ratio = image.naturalWidth && image.naturalHeight ? image.naturalWidth / image.naturalHeight : 16 / 9;
    canvas.width = 1600;
    canvas.height = Math.max(400, Math.round(canvas.width / ratio));
    const context = canvas.getContext("2d");
    context.fillStyle = "#020617";
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise((resolve, reject) => canvas.toBlob((value) => value ? resolve(value) : reject(new Error("PNG encoding failed")), "image/png"));
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "diagram-studio.png";
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  async function deleteSelection() {
    if (state.selectedType === "node") {
      const nodes = selectedUnlockedNodes();
      if (!nodes.length) return;
      const label = nodes.length === 1 ? `“${nodes[0].label}” and its connectors` : `${nodes.length} nodes and their connectors`;
      if (!window.confirm(`Delete ${label}?`)) return;
      try {
        const ids = nodes.map((node) => node.id);
        await executeCommand("nodes.delete", { node_ids_json: ids });
        const idSet = new Set(ids);
        state.nodes = state.nodes.filter((item) => !idSet.has(item.id));
        state.edges = state.edges.filter((edge) => !idSet.has(edge.source_id) && !idSet.has(edge.target_id));
        state.selectedType = null;
        state.selectedId = null;
        state.selectedNodeIds = [];
        renderAll();
        elements.canvas.focus();
        showToast(`${nodes.length} node${nodes.length === 1 ? "" : "s"} and dependent connectors deleted from SQLite.`);
      } catch (error) {
        showToast(error.message, true);
      }
      return;
    }
    if (state.selectedType === "edge") {
      const edge = state.edges.find((item) => item.id === state.selectedId);
      if (!edge) return;
      try {
        await executeCommand("edge.delete", { id: edge.id });
        state.edges = state.edges.filter((item) => item.id !== edge.id);
        state.selectedType = null;
        state.selectedId = null;
        renderAll();
        elements.canvas.focus();
        showToast("Connector deleted from SQLite.");
      } catch (error) {
        showToast(error.message, true);
      }
    }
  }

  function setSaveState(label, mode) {
    elements.saveState.textContent = label;
    elements.saveState.classList.toggle("is-saving", mode === "saving");
    elements.saveState.classList.toggle("is-error", mode === "error");
  }

  function showToast(message, error = false) {
    window.clearTimeout(state.toastTimer);
    elements.toast.textContent = message;
    elements.toast.classList.toggle("is-error", error);
    elements.toast.classList.add("is-visible");
    state.toastTimer = window.setTimeout(() => elements.toast.classList.remove("is-visible"), 2800);
  }

  function onKeyDown(event) {
    const target = event.target;
    const editing = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target?.isContentEditable;
    if (state.presenting) {
      if (event.key === "ArrowRight" || event.key === "PageDown" || event.key === " ") {
        event.preventDefault();
        moveScene(1);
      } else if (event.key === "ArrowLeft" || event.key === "PageUp") {
        event.preventDefault();
        moveScene(-1);
      } else if (event.key === "Escape") {
        exitPresentation();
      }
      return;
    }
    if (editing) return;
    const key = event.key.toLowerCase();
    const modifier = event.ctrlKey || event.metaKey;
    if (canEdit() && modifier && key === "z") {
      event.preventDefault();
      moveHistory(event.shiftKey ? "redo" : "undo");
    } else if (canEdit() && modifier && key === "y") {
      event.preventDefault();
      moveHistory("redo");
    } else if (canUseInterchange() && modifier && key === "c") {
      event.preventDefault();
      copySelection();
    } else if (canEdit() && modifier && key === "v") {
      event.preventDefault();
      pasteSelection();
    } else if (canEdit() && modifier && key === "x") {
      event.preventDefault();
      copySelection().then(() => deleteSelection());
    } else if (canEdit() && key === "n") {
      event.preventDefault();
      addNode();
    } else if (canEdit() && key === "c") {
      event.preventDefault();
      toggleConnectMode();
    } else if (key === "f") {
      event.preventDefault();
      clearSceneFocus();
      animateViewBox(calculateBounds(110));
    } else if (key === "p") {
      event.preventDefault();
      enterPresentation();
    } else if (canEdit() && ["arrowleft", "arrowright", "arrowup", "arrowdown"].includes(key) && state.selectedType === "node") {
      event.preventDefault();
      const amount = event.altKey ? 1 : 12;
      const nodes = selectedUnlockedNodes();
      const changes = event.shiftKey
        ? nodes.map((node) => geometry.transformChange(node, {
            width: node.width + (key === "arrowright" ? amount : key === "arrowleft" ? -amount : 0),
            height: node.height + (key === "arrowdown" ? amount : key === "arrowup" ? -amount : 0),
          }))
        : geometry.moveChanges(
            nodes,
            key === "arrowright" ? amount : key === "arrowleft" ? -amount : 0,
            key === "arrowdown" ? amount : key === "arrowup" ? -amount : 0,
          );
      applyNodeTransforms(changes, event.shiftKey ? `Resize ${changes.length} node${changes.length === 1 ? "" : "s"}` : `Move ${changes.length} node${changes.length === 1 ? "" : "s"}`);
    } else if (canEdit() && (event.key === "Delete" || event.key === "Backspace")) {
      event.preventDefault();
      deleteSelection();
    } else if (event.key === "Escape") {
      if (state.mode === "connect") {
        state.mode = "select";
        state.connectSourceId = null;
        updateModeUI();
        renderDiagram();
      } else {
        clearSelection();
      }
    }
  }

  boot();
})();
