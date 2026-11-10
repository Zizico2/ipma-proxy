import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { decode } from 'geobuf';
import Pbf from 'pbf';

const binary = process.env.GEOBUF_ENCODER ?? fileURLToPath(new URL('../../target/debug/examples/encode_geobuf', import.meta.url));
const point = { type: 'Point', coordinates: [-9.123456, 38.654321] };
const ring = [[-9,38],[-8,38],[-8,39],[-9,38]];
const cases = [
  point,
  { type: 'Point', coordinates: [-0.0000005, 0.0000005] },
  { type: 'Point', coordinates: [-9.123456, 38.654321, 42.5] },
  { type: 'MultiPoint', coordinates: [[-9,38],[-8,39]] },
  { type: 'LineString', coordinates: [[-9,38],[-8,39],[-10,37]] },
  { type: 'MultiLineString', coordinates: [[[-9,38],[-8,39]], [[-7,40],[-6,41],[-5,42]]] },
  { type: 'MultiLineString', coordinates: [[[-9,38],[-8,39]]] },
  { type: 'Polygon', coordinates: [ring] },
  { type: 'Polygon', coordinates: [ring, [[-8.8,38.2],[-8.2,38.2],[-8.2,38.8],[-8.8,38.2]]] },
  { type: 'MultiPolygon', coordinates: [[ring]] },
  { type: 'MultiPolygon', coordinates: [[ring], [ring, ring]] },
  { type: 'GeometryCollection', geometries: [point, {type:'Polygon',coordinates:[ring]}], bbox: [-9,38,-8,39] },
  { type: 'Feature', id: -17, geometry: point, properties: {null:null, zero:0, no:false, yes:true, n:-5, decimal:1.25, text:'Lisboa ☀', nested:{open:['Lapa',null]}, empty:[], sentinel:-99, integerFloat:1007.0}, other:'preserved' },
  { type: 'Feature', id: 'station-001', geometry: null, properties: {} },
  { type: 'Feature', geometry: null, properties: null },
  { type: 'FeatureCollection', features: [], publication_date:'2026-11-09' },
];
for (const path of process.argv.slice(2)) {
  let data = JSON.parse(readFileSync(path));
  if (Array.isArray(data)) data = { type:'FeatureCollection',features:data };
  cases.push(data);
}
cases.push({ type: 'FeatureCollection', owner:'IPMA', features: cases.filter(c=>c.type==='Feature') });
for (const filename of ['observations.geojson', 'stations.json']) {
  let data = JSON.parse(readFileSync(new URL(`../fixtures/${filename}`,import.meta.url)));
  if (Array.isArray(data)) data = { type:'FeatureCollection',features:data };
  cases.push(data);
}
function quantize(value) {
  if (!value || typeof value !== 'object') return value;
  const out = structuredClone(value);
  function walk(v) {
    if (Array.isArray(v)) return v.map(walk);
    if (typeof v === 'number') return Math.round(v*1e6)/1e6 || 0;
    return v;
  }
  if (out.coordinates) out.coordinates = walk(out.coordinates);
  if (out.geometry) out.geometry = quantize(out.geometry);
  if (out.geometries) out.geometries = out.geometries.map(quantize);
  if (out.features) out.features = out.features.map(quantize);
  return out;
}
for (const value of cases) {
  const result = spawnSync(binary, { input:JSON.stringify(value), maxBuffer:64*1024*1024 });
  assert.equal(result.status,0,result.stderr.toString());
  assert.deepEqual(decode(new Pbf(result.stdout)),quantize(value));
}
for (const invalid of [
  {type:'Point',coordinates:[1]},
  {type:'MultiPoint',coordinates:[[1,2],[1,2,3]]},
  {type:'Polygon',coordinates:[[[1,2],[2,3],[3,4],[4,5]]]},
  {type:'FeatureCollection',features:[point]},
  {type:'Point',coordinates:[1e100,2]},
]) {
  const result = spawnSync(binary,{input:JSON.stringify(invalid)});
  assert.notEqual(result.status,0,'invalid geometry was accepted');
}
console.log(`Mapbox Geobuf decoder verified ${cases.length} round trips and 5 rejected geometries.`);
