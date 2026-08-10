(() => {
  "use strict";

  const FORMAT = "org.sqlite-capsule.diagram-studio/1";
  const LIMITS = Object.freeze({ nodes: 64, edges: 128, groups: 32, layers: 16, scenes: 32 });

  function assert(condition, message) {
    if (!condition) throw new Error(message);
  }

  function array(value, name, limit) {
    assert(Array.isArray(value), `${name} must be an array`);
    assert(value.length <= limit, `${name} exceeds the ${limit} item limit`);
    return value;
  }

  function id(value, name) {
    assert(typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(value), `${name} is not a bounded stable ID`);
    return value;
  }

  function number(value, name) {
    assert(typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 100000, `${name} must be a bounded finite number`);
    return value;
  }

  function text(value, name, limit = 512) {
    assert(typeof value === "string" && value.length <= limit, `${name} must be a bounded string`);
    return value;
  }

  function object(value, name) {
    assert(value && typeof value === "object" && !Array.isArray(value), `${name} must be an object`);
    return value;
  }

  function assertBoundedTree(value, depth = 0, budget = { count: 0 }) {
    assert(depth <= 24, "Interchange document exceeds the maximum nesting depth");
    budget.count += 1;
    assert(budget.count <= 4096, "Interchange document is too structurally complex");
    if (!value || typeof value !== "object") return;
    for (const child of Array.isArray(value) ? value : Object.values(value)) assertBoundedTree(child, depth + 1, budget);
  }

  function validate(document) {
    assert(document && typeof document === "object" && !Array.isArray(document), "Interchange document must be an object");
    assertBoundedTree(document);
    assert(document.format === FORMAT, `Unsupported interchange format: ${String(document.format)}`);
    const layers = array(document.layers || [], "layers", LIMITS.layers);
    const nodes = array(document.nodes || [], "nodes", LIMITS.nodes);
    const edges = array(document.edges || [], "edges", LIMITS.edges);
    const groups = array(document.groups || [], "groups", LIMITS.groups);
    const scenes = array(document.scenes || [], "scenes", LIMITS.scenes);
    const allIds = new Set();
    const collect = (value, name) => {
      const stable = id(value, name);
      assert(!allIds.has(stable), `Duplicate stable ID: ${stable}`);
      allIds.add(stable);
      return stable;
    };
    const layerIds = new Set(layers.map((layer, index) => {
      const stable = collect(layer.id, `layers[${index}].id`);
      text(layer.name, `layers[${index}].name`, 160);
      return stable;
    }));
    const nodeIds = new Set(nodes.map((node, index) => {
      const stable = collect(node.id, `nodes[${index}].id`);
      text(node.kind, `nodes[${index}].kind`, 80);
      text(node.label, `nodes[${index}].label`, 512);
      number(node.x, `nodes[${index}].x`);
      number(node.y, `nodes[${index}].y`);
      assert(number(node.width, `nodes[${index}].width`) >= 60, "Node width is below 60");
      assert(number(node.height, `nodes[${index}].height`) >= 40, "Node height is below 40");
      if (node.layer_id) assert(layerIds.has(node.layer_id), `Node ${stable} references a missing layer`);
      if (node.style_json != null) object(node.style_json, `nodes[${index}].style_json`);
      if (node.data_json != null) object(node.data_json, `nodes[${index}].data_json`);
      return stable;
    }));
    const nodeLayers = new Map(nodes.map((node) => [node.id, node.layer_id || null]));
    edges.forEach((edge, index) => {
      collect(edge.id, `edges[${index}].id`);
      assert(nodeIds.has(edge.source_id) && nodeIds.has(edge.target_id), `Edge ${edge.id} has a dangling endpoint`);
      assert(edge.source_id !== edge.target_id, `Edge ${edge.id} is a self-loop`);
      if (edge.layer_id) assert(layerIds.has(edge.layer_id), `Edge ${edge.id} references a missing layer`);
      assert(["auto", "north", "east", "south", "west"].includes(edge.source_port || "auto"), `Edge ${edge.id} has an invalid source port`);
      assert(["auto", "north", "east", "south", "west"].includes(edge.target_port || "auto"), `Edge ${edge.id} has an invalid target port`);
      assert(["orthogonal", "direct"].includes(edge.route_mode || "orthogonal"), `Edge ${edge.id} has an invalid route mode`);
      array(edge.waypoints_json || [], `edges[${index}].waypoints_json`, 64).forEach((point, pointIndex) => {
        object(point, `edges[${index}].waypoints_json[${pointIndex}]`);
        number(point.x, `edges[${index}].waypoints_json[${pointIndex}].x`);
        number(point.y, `edges[${index}].waypoints_json[${pointIndex}].y`);
      });
    });
    groups.forEach((group, index) => {
      collect(group.id, `groups[${index}].id`);
      text(group.name, `groups[${index}].name`, 160);
      if (group.layer_id) assert(layerIds.has(group.layer_id), `Group ${group.id} references a missing layer`);
      const members = array(group.member_ids || [], `groups[${index}].member_ids`, LIMITS.nodes);
      assert(new Set(members).size === members.length && members.every((member) => nodeIds.has(member)), `Group ${group.id} has invalid membership`);
      if (group.layer_id) assert(members.every((member) => nodeLayers.get(member) === group.layer_id), `Group ${group.id} crosses semantic layers`);
    });
    scenes.forEach((scene, index) => {
      collect(scene.id, `scenes[${index}].id`);
      text(scene.title, `scenes[${index}].title`, 240);
      text(scene.narrative || "", `scenes[${index}].narrative`, 2000);
      const viewport = object(scene.viewport_json, `scenes[${index}].viewport_json`);
      ["x", "y", "width", "height"].forEach((key) => number(viewport[key], `scenes[${index}].viewport_json.${key}`));
      array(scene.focus_json || [], `scenes[${index}].focus_json`, LIMITS.nodes).forEach((nodeId) => assert(nodeIds.has(nodeId), `Scene ${scene.id} has a dangling focus ID`));
      array(scene.overrides_json || [], `scenes[${index}].overrides_json`, LIMITS.nodes).forEach((override, overrideIndex) => {
        object(override, `scenes[${index}].overrides_json[${overrideIndex}]`);
        assert(nodeIds.has(override.node_id), `Scene ${scene.id} has a dangling override ID`);
        for (const key of ["x", "y", "width", "height"]) if (override[key] != null) number(override[key], `scenes[${index}].overrides_json[${overrideIndex}].${key}`);
        if (override.visible != null) assert(override.visible === 0 || override.visible === 1 || typeof override.visible === "boolean", `Scene ${scene.id} has an invalid override visibility`);
        if (override.style_json != null) object(override.style_json, `scenes[${index}].overrides_json[${overrideIndex}].style_json`);
      });
    });
    return structuredClone({ ...document, layers, nodes, edges, groups, scenes });
  }

  function remap(document, suffix) {
    const validated = validate(document);
    const safeSuffix = String(suffix).replace(/[^A-Za-z0-9._:-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48) || "copy";
    const mapping = new Map();
    const mapped = (value) => {
      if (!mapping.has(value)) mapping.set(value, `${value}-${safeSuffix}`.slice(0, 128));
      return mapping.get(value);
    };
    const layers = validated.layers.map((layer) => ({ ...layer, id: mapped(layer.id) }));
    const nodes = validated.nodes.map((node) => ({ ...node, id: mapped(node.id), layer_id: node.layer_id ? mapped(node.layer_id) : undefined }));
    const edges = validated.edges.map((edge) => ({ ...edge, id: mapped(edge.id), source_id: mapped(edge.source_id), target_id: mapped(edge.target_id), layer_id: edge.layer_id ? mapped(edge.layer_id) : undefined }));
    const groups = validated.groups.map((group) => ({ ...group, id: mapped(group.id), layer_id: group.layer_id ? mapped(group.layer_id) : undefined, member_ids: group.member_ids.map(mapped) }));
    const scenes = validated.scenes.map((scene) => ({ ...scene, id: mapped(scene.id), focus_json: (scene.focus_json || []).map(mapped), overrides_json: (scene.overrides_json || []).map((override) => ({ ...override, node_id: mapped(override.node_id) })) }));
    const remapped = { ...validated, layers, nodes, edges, groups, scenes };
    validate(remapped);
    return { ...remapped, id_mapping: Object.fromEntries(mapping) };
  }

  function escape(value) {
    return String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
  }

  function toSvg(document, geometry) {
    const validated = validate(document);
    const orderedLayers = [...validated.layers]
      .filter((layer) => layer.visible !== 0)
      .sort((a, b) => (a.position || 0) - (b.position || 0) || a.id.localeCompare(b.id));
    const visibleLayerIds = new Set(orderedLayers.map((layer) => layer.id));
    const layerPosition = new Map(orderedLayers.map((layer, index) => [layer.id, index]));
    const nodes = validated.nodes
      .filter((node) => !node.layer_id || visibleLayerIds.has(node.layer_id))
      .sort((a, b) => (layerPosition.get(a.layer_id) || 0) - (layerPosition.get(b.layer_id) || 0) || (a.z_index || 0) - (b.z_index || 0) || a.id.localeCompare(b.id));
    const bounds = geometry.selectionBounds(nodes) || { x: 0, y: 0, width: 800, height: 600 };
    const padding = 48;
    const nodeMap = new Map(nodes.map((node) => [node.id, node]));
    const edges = validated.edges
      .filter((edge) => (!edge.layer_id || visibleLayerIds.has(edge.layer_id)) && nodeMap.has(edge.source_id) && nodeMap.has(edge.target_id))
      .sort((a, b) => (layerPosition.get(a.layer_id) || 0) - (layerPosition.get(b.layer_id) || 0) || a.id.localeCompare(b.id));
    const edgeSvg = (edge) => {
      const source = nodeMap.get(edge.source_id);
      const target = nodeMap.get(edge.target_id);
      const route = geometry.routeOrthogonal(source, target, nodes.filter((node) => node.id !== source.id && node.id !== target.id), {
        sourcePort: edge.source_port === "auto" ? undefined : edge.source_port,
        targetPort: edge.target_port === "auto" ? undefined : edge.target_port,
      });
      const path = edge.route_mode === "direct"
        ? `M ${route.points[0].x} ${route.points[0].y} L ${route.points.at(-1).x} ${route.points.at(-1).y}`
        : route.path;
      return `<path class="edge" d="${escape(path)}"/>`;
    };
    const nodeSvg = (node) => {
      const shape = node.data_json?.shape || "rounded-rectangle";
      const path = geometry.shapePath(shape, node.width, node.height);
      return `<g transform="translate(${node.x} ${node.y})"><path class="node" d="${escape(path)}"/><text x="18" y="32">${escape(node.label)}</text></g>`;
    };
    const layerIds = [...orderedLayers.map((layer) => layer.id), null];
    const contents = layerIds.map((layerId) => {
      const layer = orderedLayers.find((item) => item.id === layerId);
      const edgeBodies = edges.filter((edge) => (edge.layer_id || null) === layerId).map(edgeSvg).join("");
      const nodeBodies = nodes.filter((node) => (node.layer_id || null) === layerId).map(nodeSvg).join("");
      if (!edgeBodies && !nodeBodies) return "";
      return `<g${layer ? ` id="${escape(layer.id)}" aria-label="${escape(layer.name)} layer"` : ""}>${edgeBodies}${nodeBodies}</g>`;
    }).join("");
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${bounds.x - padding} ${bounds.y - padding} ${bounds.width + padding * 2} ${bounds.height + padding * 2}" role="img"><title>${escape(validated.title || "Diagram Studio export")}</title><desc>${escape(validated.description || "Offline semantic diagram export")}</desc><style>.node{fill:#172554;stroke:#60a5fa;stroke-width:2}.edge{fill:none;stroke:#64748b;stroke-width:2.5;marker-end:url(#arrow)}text{fill:#f8fafc;font:600 16px system-ui,sans-serif}.background{fill:#020617}</style><defs><marker id="arrow" markerWidth="10" markerHeight="10" refX="9" refY="5" orient="auto"><path d="M0 0L10 5L0 10Z" fill="#64748b"/></marker></defs><rect class="background" x="${bounds.x - padding}" y="${bounds.y - padding}" width="${bounds.width + padding * 2}" height="${bounds.height + padding * 2}"/>${contents}</svg>`;
  }

  globalThis.DiagramStudioInterchange = Object.freeze({ FORMAT, LIMITS, remap, toSvg, validate });
})();
