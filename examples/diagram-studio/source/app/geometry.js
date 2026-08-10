(() => {
  "use strict";

  const MIN_WIDTH = 60;
  const MIN_HEIGHT = 40;

  function finite(value, fallback = 0) {
    const number = Number(value);
    return Number.isFinite(number) ? number : fallback;
  }

  function snap(value, step = 12) {
    return Math.round(finite(value) / step) * step;
  }

  function frame(node) {
    return {
      x: finite(node.x),
      y: finite(node.y),
      width: Math.max(MIN_WIDTH, finite(node.width, MIN_WIDTH)),
      height: Math.max(MIN_HEIGHT, finite(node.height, MIN_HEIGHT)),
    };
  }

  function selectionBounds(nodes) {
    if (!nodes.length) return null;
    const left = Math.min(...nodes.map((node) => finite(node.x)));
    const top = Math.min(...nodes.map((node) => finite(node.y)));
    const right = Math.max(...nodes.map((node) => finite(node.x) + finite(node.width)));
    const bottom = Math.max(...nodes.map((node) => finite(node.y) + finite(node.height)));
    return { x: left, y: top, width: right - left, height: bottom - top, right, bottom };
  }

  function transformChange(node, next) {
    const from = frame(node);
    const to = {
      x: finite(next.x, from.x),
      y: finite(next.y, from.y),
      width: Math.max(MIN_WIDTH, finite(next.width, from.width)),
      height: Math.max(MIN_HEIGHT, finite(next.height, from.height)),
    };
    return { id: node.id, from, to };
  }

  function moveChanges(nodes, dx, dy) {
    return nodes.map((node) => transformChange(node, {
      x: finite(node.x) + dx,
      y: finite(node.y) + dy,
    }));
  }

  function alignChanges(nodes, mode) {
    const bounds = selectionBounds(nodes);
    if (!bounds || nodes.length < 2) return [];
    return nodes.map((node) => {
      let x = node.x;
      let y = node.y;
      if (mode === "left") x = bounds.x;
      if (mode === "center") x = bounds.x + (bounds.width - node.width) / 2;
      if (mode === "right") x = bounds.right - node.width;
      if (mode === "top") y = bounds.y;
      if (mode === "middle") y = bounds.y + (bounds.height - node.height) / 2;
      if (mode === "bottom") y = bounds.bottom - node.height;
      return transformChange(node, { x: snap(x, 1), y: snap(y, 1) });
    });
  }

  function distributeChanges(nodes, axis) {
    if (nodes.length < 3) return [];
    const horizontal = axis === "horizontal";
    const ordered = [...nodes].sort((a, b) => {
      const av = horizontal ? a.x : a.y;
      const bv = horizontal ? b.x : b.y;
      return av - bv || a.id.localeCompare(b.id);
    });
    const first = ordered[0];
    const last = ordered[ordered.length - 1];
    const span = horizontal ? last.x - first.x : last.y - first.y;
    return ordered.map((node, index) => transformChange(node, horizontal
      ? { x: snap(first.x + span * index / (ordered.length - 1), 1) }
      : { y: snap(first.y + span * index / (ordered.length - 1), 1) }));
  }

  function shapePath(kind, width, height) {
    const w = Math.max(MIN_WIDTH, finite(width, MIN_WIDTH));
    const h = Math.max(MIN_HEIGHT, finite(height, MIN_HEIGHT));
    if (kind === "ellipse") {
      return `M ${w / 2} 0 A ${w / 2} ${h / 2} 0 1 1 ${w / 2} ${h} A ${w / 2} ${h / 2} 0 1 1 ${w / 2} 0 Z`;
    }
    if (kind === "diamond") return `M ${w / 2} 0 L ${w} ${h / 2} L ${w / 2} ${h} L 0 ${h / 2} Z`;
    if (kind === "note") {
      const fold = Math.min(30, w * 0.22, h * 0.3);
      return `M 0 0 H ${w - fold} L ${w} ${fold} V ${h} H 0 Z M ${w - fold} 0 V ${fold} H ${w}`;
    }
    return `M 0 0 H ${w} V ${h} H 0 Z`;
  }

  function preferredPort(node, toward) {
    const center = { x: node.x + node.width / 2, y: node.y + node.height / 2 };
    const dx = toward.x - center.x;
    const dy = toward.y - center.y;
    if (Math.abs(dx) >= Math.abs(dy)) return dx >= 0 ? "east" : "west";
    return dy >= 0 ? "south" : "north";
  }

  function portPoint(node, port) {
    if (port === "north") return { x: node.x + node.width / 2, y: node.y };
    if (port === "south") return { x: node.x + node.width / 2, y: node.y + node.height };
    if (port === "west") return { x: node.x, y: node.y + node.height / 2 };
    return { x: node.x + node.width, y: node.y + node.height / 2 };
  }

  function segmentHitsRect(a, b, rect, padding = 18) {
    const left = rect.x - padding;
    const right = rect.x + rect.width + padding;
    const top = rect.y - padding;
    const bottom = rect.y + rect.height + padding;
    if (a.x === b.x) return a.x > left && a.x < right && Math.max(a.y, b.y) > top && Math.min(a.y, b.y) < bottom;
    if (a.y === b.y) return a.y > top && a.y < bottom && Math.max(a.x, b.x) > left && Math.min(a.x, b.x) < right;
    return false;
  }

  function cleanPoints(points) {
    return points.filter((point, index) => {
      if (index && point.x === points[index - 1].x && point.y === points[index - 1].y) return false;
      if (index > 0 && index < points.length - 1) {
        const before = points[index - 1];
        const after = points[index + 1];
        if ((before.x === point.x && point.x === after.x) || (before.y === point.y && point.y === after.y)) return false;
      }
      return true;
    });
  }

  function routeOrthogonal(source, target, obstacles = [], options = {}) {
    const targetCenter = { x: target.x + target.width / 2, y: target.y + target.height / 2 };
    const sourceCenter = { x: source.x + source.width / 2, y: source.y + source.height / 2 };
    const sourcePort = options.sourcePort || preferredPort(source, targetCenter);
    const targetPort = options.targetPort || preferredPort(target, sourceCenter);
    const start = portPoint(source, sourcePort);
    const end = portPoint(target, targetPort);
    const gap = 36;
    const sx = sourcePort === "east" ? start.x + gap : sourcePort === "west" ? start.x - gap : start.x;
    const sy = sourcePort === "south" ? start.y + gap : sourcePort === "north" ? start.y - gap : start.y;
    const tx = targetPort === "east" ? end.x + gap : targetPort === "west" ? end.x - gap : end.x;
    const ty = targetPort === "south" ? end.y + gap : targetPort === "north" ? end.y - gap : end.y;
    const candidates = [
      [start, { x: sx, y: sy }, { x: tx, y: sy }, { x: tx, y: ty }, end],
      [start, { x: sx, y: sy }, { x: sx, y: ty }, { x: tx, y: ty }, end],
    ];
    const above = Math.min(start.y, end.y, ...obstacles.map((item) => item.y)) - 42;
    const below = Math.max(start.y, end.y, ...obstacles.map((item) => item.y + item.height)) + 42;
    candidates.push(
      [start, { x: sx, y: sy }, { x: sx, y: above }, { x: tx, y: above }, { x: tx, y: ty }, end],
      [start, { x: sx, y: sy }, { x: sx, y: below }, { x: tx, y: below }, { x: tx, y: ty }, end],
    );
    const cleaned = candidates.map(cleanPoints);
    const unobstructed = cleaned.find((points) => points.slice(1).every((point, index) =>
      obstacles.every((obstacle) => !segmentHitsRect(points[index], point, obstacle))));
    const usable = unobstructed || cleaned[0];
    return {
      points: usable,
      sourcePort,
      targetPort,
      fallback: !unobstructed,
      path: usable.map((point, index) => `${index ? "L" : "M"} ${point.x} ${point.y}`).join(" "),
    };
  }

  function gridLayout(nodes, options = {}) {
    const ordered = [...nodes].sort((a, b) => a.id.localeCompare(b.id));
    const columns = Math.max(1, Math.ceil(Math.sqrt(ordered.length)));
    const originX = finite(options.x, 120);
    const originY = finite(options.y, 120);
    const cellWidth = Math.max(finite(options.cellWidth, 320), ...ordered.map((node) => finite(node.width) + 80));
    const cellHeight = Math.max(finite(options.cellHeight, 220), ...ordered.map((node) => finite(node.height) + 80));
    return ordered.map((node, index) => transformChange(node, {
      x: originX + (index % columns) * cellWidth,
      y: originY + Math.floor(index / columns) * cellHeight,
    }));
  }

  function directionalLayout(nodes, options = {}) {
    const ordered = [...nodes].sort((a, b) => a.id.localeCompare(b.id));
    const originX = finite(options.x, 120);
    const originY = finite(options.y, 160);
    const gapX = Math.max(finite(options.gapX, 310), ...ordered.map((node) => finite(node.width) + 80));
    const gapY = Math.max(finite(options.gapY, 190), ...ordered.map((node) => finite(node.height) + 70));
    return ordered.map((node, index) => transformChange(node, {
      x: originX + index * gapX,
      y: originY + (index % 2) * gapY,
    }));
  }

  function layeredLayout(nodes, edges = [], options = {}) {
    const ids = new Set(nodes.map((node) => node.id));
    const incoming = new Map(nodes.map((node) => [node.id, 0]));
    const outgoing = new Map(nodes.map((node) => [node.id, []]));
    for (const edge of edges.filter((item) => ids.has(item.source_id) && ids.has(item.target_id))) {
      outgoing.get(edge.source_id).push(edge.target_id);
      incoming.set(edge.target_id, incoming.get(edge.target_id) + 1);
    }
    for (const targets of outgoing.values()) targets.sort();
    const queue = [...nodes.map((node) => node.id).filter((id) => incoming.get(id) === 0)].sort();
    const level = new Map(queue.map((id) => [id, 0]));
    while (queue.length) {
      const id = queue.shift();
      for (const target of outgoing.get(id)) {
        level.set(target, Math.max(level.get(target) || 0, (level.get(id) || 0) + 1));
        incoming.set(target, incoming.get(target) - 1);
        if (incoming.get(target) === 0) queue.push(target);
      }
      queue.sort();
    }
    for (const id of [...ids].sort()) if (!level.has(id)) level.set(id, 0);
    const rows = new Map();
    for (const node of [...nodes].sort((a, b) => a.id.localeCompare(b.id))) {
      const value = level.get(node.id);
      const row = rows.get(value) || [];
      row.push(node);
      rows.set(value, row);
    }
    const changes = [];
    const gapX = Math.max(finite(options.gapX, 340), ...nodes.map((node) => finite(node.width) + 80));
    const gapY = Math.max(finite(options.gapY, 210), ...nodes.map((node) => finite(node.height) + 70));
    for (const [column, row] of [...rows.entries()].sort((a, b) => a[0] - b[0])) {
      row.forEach((node, index) => changes.push(transformChange(node, {
        x: finite(options.x, 120) + column * gapX,
        y: finite(options.y, 120) + index * gapY,
      })));
    }
    return changes.sort((a, b) => a.id.localeCompare(b.id));
  }

  globalThis.DiagramStudioGeometry = Object.freeze({
    MIN_WIDTH,
    MIN_HEIGHT,
    alignChanges,
    distributeChanges,
    directionalLayout,
    frame,
    gridLayout,
    layeredLayout,
    moveChanges,
    portPoint,
    routeOrthogonal,
    selectionBounds,
    shapePath,
    snap,
    transformChange,
  });
})();
