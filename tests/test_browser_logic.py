from __future__ import annotations

import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GEOMETRY = ROOT / "examples" / "diagram-studio" / "source" / "app" / "geometry.js"
INTERCHANGE = ROOT / "examples" / "diagram-studio" / "source" / "app" / "interchange.js"


class BrowserLogicTests(unittest.TestCase):
    def run_node(self, body: str, *, interchange: bool = False) -> dict[str, object]:
        sources = [GEOMETRY, INTERCHANGE] if interchange else [GEOMETRY]
        prelude = "\n".join(
            f'require("vm").runInThisContext(require("fs").readFileSync({json.dumps(str(path))}, "utf8"));'
            for path in sources
        )
        result = subprocess.run(
            ["node", "-e", f'{prelude}\n{body}'],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_geometry_is_deterministic_for_shapes_transforms_and_layouts(self) -> None:
        report = self.run_node(
            r'''
const g = DiagramStudioGeometry;
const nodes = [
  {id:"c",x:200,y:80,width:120,height:60},
  {id:"a",x:0,y:20,width:100,height:50},
  {id:"b",x:90,y:180,width:80,height:40},
];
const shapes = ["rounded-rectangle","rectangle","ellipse","diamond","pill","note","container"];
const paths = Object.fromEntries(shapes.map(kind => [kind, g.shapePath(kind, 140, 80)]));
const left = g.alignChanges(nodes, "left");
const distributed = g.distributeChanges(nodes, "horizontal");
const grid = g.gridLayout(nodes);
const gridReversed = g.gridLayout([...nodes].reverse());
const directional = g.directionalLayout(nodes);
const layered = g.layeredLayout(nodes, [
  {source_id:"a",target_id:"b"},
  {source_id:"b",target_id:"c"},
]);
console.log(JSON.stringify({
  shapeCount:Object.values(paths).filter(value => typeof value === "string" && value.startsWith("M ")).length,
  leftX:left.map(change => change.to.x),
  distributedX:distributed.map(change => change.to.x),
  deterministic:JSON.stringify(grid) === JSON.stringify(gridReversed),
  directionalIds:directional.map(change => change.id),
  layered:layered.map(change => [change.id, change.to.x]),
}));
'''
        )
        self.assertEqual(report["shapeCount"], 7)
        self.assertEqual(report["leftX"], [0, 0, 0])
        self.assertEqual(report["distributedX"], [0, 100, 200])
        self.assertTrue(report["deterministic"])
        self.assertEqual(report["directionalIds"], ["a", "b", "c"])
        self.assertEqual(report["layered"], [["a", 120], ["b", 460], ["c", 800]])

    def test_orthogonal_router_avoids_a_fixture_obstacle(self) -> None:
        report = self.run_node(
            r'''
const g = DiagramStudioGeometry;
const source = {id:"a",x:0,y:0,width:100,height:100};
const target = {id:"b",x:400,y:0,width:100,height:100};
const obstacle = {id:"block",x:190,y:10,width:100,height:80};
const route = g.routeOrthogonal(source, target, [obstacle], {sourcePort:"east",targetPort:"west"});
const blocked = g.routeOrthogonal(source, target, [
  {id:"source-wall",x:112,y:-180,width:48,height:440},
  {id:"target-wall",x:340,y:-180,width:48,height:440},
], {sourcePort:"east",targetPort:"west"});
const bounded = Array.from({length:64}, (_, index) => ({id:`n-${String(index).padStart(2,"0")}`,x:(index%8)*180,y:Math.floor(index/8)*130,width:100,height:64}));
const layoutA = g.gridLayout(bounded);
const layoutB = g.gridLayout([...bounded].reverse());
console.log(JSON.stringify({fallback:route.fallback,path:route.path,points:route.points,blockedFallback:blocked.fallback,boundedDeterministic:JSON.stringify(layoutA)===JSON.stringify(layoutB),boundedCount:layoutA.length}));
'''
        )
        self.assertFalse(report["fallback"])
        self.assertIn("M 100 50", report["path"])
        self.assertTrue(any(point["y"] < -20 or point["y"] > 120 for point in report["points"]))
        self.assertTrue(report["blockedFallback"])
        self.assertTrue(report["boundedDeterministic"])
        self.assertEqual(report["boundedCount"], 64)

    def test_interchange_rejects_dangling_and_oversized_input_and_exports_safe_svg(self) -> None:
        report = self.run_node(
            r'''
const i = DiagramStudioInterchange;
const base = {
  format:i.FORMAT,
  title:"A <safe> diagram",
  description:"Offline export",
  layers:[
    {id:"layer-content",name:"Content"},
    {id:"layer-connectors",name:"Connectors"},
  ],
  nodes:[
    {id:"n1",layer_id:"layer-content",kind:"note",label:"<script>alert(1)</script>",x:0,y:0,width:120,height:80,style_json:{},data_json:{shape:"note"}},
    {id:"n2",layer_id:"layer-content",kind:"note",label:"Two",x:300,y:0,width:120,height:80,style_json:{},data_json:{}},
  ],
  edges:[{id:"e1",layer_id:"layer-connectors",source_id:"n1",target_id:"n2",source_port:"east",target_port:"west",route_mode:"orthogonal",waypoints_json:[]}],
  groups:[{id:"g1",layer_id:"layer-content",name:"Pair",member_ids:["n1","n2"]}],
  scenes:[{id:"s1",title:"Overview",narrative:"",viewport_json:{x:0,y:0,width:800,height:600},focus_json:["n1"],overrides_json:[{node_id:"n2",x:320,visible:1}]}],
};
const valid = i.validate(base);
const mapped = i.remap(base, "paste 1");
const svg = i.toSvg(base, DiagramStudioGeometry);
let dangling = false;
try { i.validate({...base, edges:[{...base.edges[0], target_id:"missing"}]}); } catch { dangling = true; }
let danglingOverride = false;
try { i.validate({...base, scenes:[{...base.scenes[0], overrides_json:[{node_id:"missing"}]}]}); } catch { danglingOverride = true; }
let oversized = false;
try { i.validate({...base, nodes:Array.from({length:65}, (_, index) => ({...base.nodes[0], id:`n${index + 10}`})), edges:[], groups:[], scenes:[]}); } catch { oversized = true; }
let tooDeep = false;
try {
  let nested = {};
  for (let depth = 0; depth < 30; depth += 1) nested = {nested};
  i.validate({...base, nodes:[{...base.nodes[0], data_json:nested}], edges:[], groups:[], scenes:[]});
} catch { tooDeep = true; }
console.log(JSON.stringify({
  nodeCount:valid.nodes.length,
  mappedIds:mapped.nodes.map(node => node.id),
  dangling,
  danglingOverride,
  oversized,
  tooDeep,
  svgStart:svg.startsWith("<svg "),
  escaped:svg.includes("&lt;script&gt;alert(1)&lt;/script&gt;"),
  activeContent:/<script|<foreignObject|url\(https?:/i.test(svg),
}));
''',
            interchange=True,
        )
        self.assertEqual(report["nodeCount"], 2)
        self.assertEqual(report["mappedIds"], ["n1-paste-1", "n2-paste-1"])
        self.assertTrue(report["dangling"])
        self.assertTrue(report["danglingOverride"])
        self.assertTrue(report["oversized"])
        self.assertTrue(report["tooDeep"])
        self.assertTrue(report["svgStart"])
        self.assertTrue(report["escaped"])
        self.assertFalse(report["activeContent"])


if __name__ == "__main__":
    unittest.main()
