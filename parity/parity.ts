/**
 * Activation parity: `neat_ai_predict` against `@stsoftware/neat-ai`'s own
 * `creature.activate` — the call GRQ's daily scoring and its TypeScript
 * historical inference make (issue #4).
 *
 * Builds a GRQ-format observation archive of deterministic pseudo-random rows
 * for the given creature, runs the binary over it, activates every row through
 * neat-ai, and compares the two output by output. Prints the agreement and
 * exits non-zero when any output differs by more than `--tolerance`.
 *
 * Usage:
 *   deno run --no-prompt --allow-read --allow-write --allow-run --allow-env \
 *     --allow-net=jsr.io parity.ts --creature <json> --binary <neat_ai_predict> \
 *     [--rows 2000] [--extra-inputs 0] [--tolerance 0] [--work <dir>]
 */
import { Creature } from "@stsoftware/neat-ai";
import { crypto } from "@std/crypto";
import { encodeHex } from "@std/encoding/hex";

interface Args {
  creature: string;
  binary: string;
  rows: number;
  extraInputs: number;
  tolerance: number;
  work: string;
}

function parseArgs(argv: string[]): Args {
  const value = (flag: string): string | undefined => {
    const at = argv.indexOf(flag);
    return at >= 0 ? argv[at + 1] : undefined;
  };
  const creature = value("--creature");
  const binary = value("--binary");
  if (!creature || !binary) {
    throw new Error("--creature and --binary are required");
  }
  return {
    creature,
    binary,
    rows: Number(value("--rows") ?? "2000"),
    extraInputs: Number(value("--extra-inputs") ?? "0"),
    tolerance: Number(value("--tolerance") ?? "0"),
    work: value("--work") ??
      Deno.makeTempDirSync({ prefix: "predict-parity-" }),
  };
}

/** mulberry32: a small deterministic PRNG, so a failing run reproduces. */
function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

async function sha256Hex(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  return encodeHex(await crypto.subtle.digest("SHA-256", bytes));
}

const EXTENSION = 116;
const FINGERPRINT = "parityfingerprnt";
const SYMBOLS = ["AAPL", "AMZN", "BHP", "CBA", "MSFT", "NAB", "TSLA", "WBC"];

async function buildArchive(
  root: string,
  width: number,
  count: number,
): Promise<
  {
    fpRoot: string;
    rows: { symbol: string; date: string; values: Float32Array }[];
  }
> {
  const fpRoot = `${root}/${EXTENSION}/${FINGERPRINT}`;
  Deno.mkdirSync(`${fpRoot}/datasets`, { recursive: true });
  const next = prng(4780);
  const rows: { symbol: string; date: string; values: Float32Array }[] = [];
  for (let i = 0; i < count; i++) {
    const values = new Float32Array(width);
    for (let j = 0; j < width; j++) values[j] = next() * 2 - 1;
    // Spread rows across symbols and days of one month: one partition per prefix.
    const symbol = SYMBOLS[i % SYMBOLS.length];
    const day = 1 + Math.floor(i / SYMBOLS.length) % 28;
    const month = 1 + Math.floor(i / (SYMBOLS.length * 28)) % 12;
    const year = 2007 + Math.floor(i / (SYMBOLS.length * 28 * 12));
    const date = `${year}-${String(month).padStart(2, "0")}-${
      String(day).padStart(2, "0")
    }`;
    rows.push({ symbol, date, values });
  }

  const partitions = new Map<string, typeof rows>();
  for (const row of rows) {
    const key = `${row.date.slice(0, 4)}/${row.date.slice(5, 7)}/${
      row.symbol[0]
    }`;
    const bucket = partitions.get(key);
    if (bucket) bucket.push(row);
    else partitions.set(key, [row]);
  }
  const shards = [];
  let chunk = 0;
  for (const [key, part] of partitions) {
    const [year, month, prefix] = key.split("/");
    const chunkId = String(chunk++).padStart(6, "0");
    const dir = `${fpRoot}/${key}`;
    Deno.mkdirSync(dir, { recursive: true });
    const payload = new Uint8Array(part.length * width * 4);
    const view = new DataView(payload.buffer);
    part.forEach((row, r) =>
      row.values.forEach((v, c) =>
        view.setFloat32((r * width + c) * 4, v, true)
      )
    );
    const sha = await sha256Hex(payload);
    Deno.writeFileSync(`${dir}/${chunkId}.bin`, payload);
    Deno.writeTextFileSync(
      `${dir}/${chunkId}.index.json`,
      JSON.stringify({
        schema: "grq.observations.shard/1",
        byteOrder: "little-endian",
        observationExtension: EXTENSION,
        inputCount: width,
        featureFingerprint: FINGERPRINT,
        semanticIdentity: {},
        prefix,
        year: Number(year),
        month: Number(month),
        chunkId,
        shardFile: `${chunkId}.bin`,
        shardBytes: payload.length,
        shardSha256: sha,
        rowCount: part.length,
        dates: [],
        generatorRevision: "parity",
        createdUTC: new Date().toISOString(),
        rows: part.map((row, r) => ({
          symbol: row.symbol,
          date: row.date,
          row: r,
        })),
      }),
    );
    shards.push({
      path: `${key}/${chunkId}.bin`,
      sha256: sha,
      bytes: payload.length,
      rowCount: part.length,
      prefix,
      year: Number(year),
      month: Number(month),
      chunkId,
    });
  }
  Deno.writeTextFileSync(
    `${fpRoot}/datasets/parity.json`,
    JSON.stringify({
      schema: "grq.observations.dataset/1",
      datasetId: "parity",
      createdUTC: new Date().toISOString(),
      byteOrder: "little-endian",
      observationExtension: EXTENSION,
      inputCount: width,
      featureFingerprint: FINGERPRINT,
      semanticIdentity: {},
      generatorRevision: "parity",
      shards,
      coveredDates: [],
      replacements: [],
      exclusions: [],
      missingWork: [],
      previousDatasetId: null,
      totals: {
        shardCount: shards.length,
        rowCount: rows.length,
        shardBytes: 0,
      },
    }),
  );
  Deno.writeTextFileSync(
    `${fpRoot}/latest.json`,
    JSON.stringify({ datasetId: "parity" }),
  );
  return { fpRoot, rows };
}

function readPredictions(out: string): Map<string, Float64Array> {
  const manifest = JSON.parse(Deno.readTextFileSync(`${out}/manifest.json`));
  const outputCount: number = manifest.identity.outputCount;
  const values = new Map<string, Float64Array>();
  for (const partition of manifest.partitions) {
    const [year, month, prefix] = partition.path.split("/");
    const dir = `${out}/${year}/${month}`;
    const index = JSON.parse(
      Deno.readTextFileSync(`${dir}/${prefix}.index.json`),
    );
    const bytes = Deno.readFileSync(`${dir}/${prefix}.bin`);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    for (const row of index.rows) {
      const outputs = new Float64Array(outputCount);
      for (let i = 0; i < outputCount; i++) {
        outputs[i] = view.getFloat64((row.row * outputCount + i) * 8, true);
      }
      values.set(`${row.symbol} ${row.date}`, outputs);
    }
  }
  return values;
}

async function main(): Promise<number> {
  const args = parseArgs(Deno.args);
  const json = JSON.parse(Deno.readTextFileSync(args.creature));
  const width = json.input + args.extraInputs;
  const { fpRoot, rows } = await buildArchive(
    `${args.work}/archive`,
    width,
    args.rows,
  );

  const out = `${args.work}/predictions`;
  const run = await new Deno.Command(args.binary, {
    args: [
      "predict",
      "--creature",
      args.creature,
      "--archive",
      fpRoot,
      "--output",
      out,
    ],
    stdout: "inherit",
    stderr: "inherit",
  }).output();
  if (!run.success) {
    console.error(`❌ ${args.binary} exited ${run.code}`);
    return 1;
  }
  const predicted = readPredictions(out);

  const creature = Creature.fromJSON(json);
  let identical = 0;
  let compared = 0;
  let worst = 0;
  let worstKey = "";
  for (const row of rows) {
    const key = `${row.symbol} ${row.date}`;
    const ours = predicted.get(key);
    if (!ours) {
      console.error(`❌ no prediction for ${key}`);
      return 1;
    }
    const theirs = creature.activate(row.values.subarray(0, json.input));
    for (let i = 0; i < ours.length; i++) {
      compared++;
      // neat-ai activates in f32; compare after rounding ours back to f32.
      const a = Math.fround(ours[i]);
      const b = Math.fround(theirs[i]);
      if (Object.is(a, b)) identical++;
      const diff = Math.abs(a - b);
      if (diff > worst) {
        worst = diff;
        worstKey = `${key} output ${i}: ours ${a} neat-ai ${b}`;
      }
    }
  }
  console.log(
    `parity: ${identical}/${compared} outputs bit-identical to @stsoftware/neat-ai creature.activate; ` +
      `max |diff| ${worst}${worstKey ? ` (${worstKey})` : ""}`,
  );
  if (worst > args.tolerance) {
    console.error(
      `❌ max |diff| ${worst} exceeds --tolerance ${args.tolerance}`,
    );
    return 1;
  }
  return 0;
}

Deno.exit(await main());
