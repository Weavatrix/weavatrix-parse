// Reproducible current-vs-baseline parser gate.
//
// The same regression-bench source is compiled against both parser trees.
// Corpus paths come from tools/real-corpus.json and remain relative in Git.
// A detached baseline worktree and both Cargo target directories live under
// one verified OS-temp directory and are removed on exit.
//
//   node tools/compare-parser-revisions.mjs \
//     --baseline HEAD --samples 3 --max-slowdown-pct 10 \
//     --min-regression-ms 1 \
//     --out target/parser-regression.json
import {
    existsSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
    buildBench,
    execBench,
    fingerprintChanges,
    git,
    prepareBench,
} from './parser-comparison-benchmark.mjs'
import { parseArgs } from './parser-comparison-cli.mjs'
import { snapshotCorpus } from './parser-comparison-corpus.mjs'
import {
    assertDeterministicFacts,
    compareFacts,
    compareThroughput,
    parseSnapshot,
} from './parser-comparison-results.mjs'

const projectRoot = fileURLToPath(new URL('..', import.meta.url))
const options = parseArgs(process.argv.slice(2))
if (!options.out) throw new Error('--out is required')
const manifestPath = resolve(projectRoot, options.manifest)
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8').replace(/^\uFEFF/, ''))
if (manifest.schema !== 'weavatrix.parse-real-corpus.v1') {
    throw new Error(`unsupported corpus manifest: ${manifest.schema}`)
}
const corpusEntries = manifest.repositories.map((entry) => typeof entry === 'string'
    ? {repository: entry, scan: entry}
    : entry)
const sourceRoots = corpusEntries.map((entry) => resolve(projectRoot, entry.scan))
const missing = sourceRoots.filter((path) => !existsSync(path))
if (missing.length) throw new Error(`missing corpus roots: ${missing.join(', ')}`)

const scratch = mkdtempSync(join(tmpdir(), 'weavatrix-parse-regression-'))
const baselineRoot = join(scratch, 'baseline')
let worktreeAdded = false
try {
    const snapshot = snapshotCorpus(
        sourceRoots,
        join(scratch, 'corpus'),
        manifest.capBytesPerLanguage,
    )
    const roots = snapshot.roots
    git(['worktree', 'add', '--detach', baselineRoot, options.baseline], projectRoot)
    worktreeAdded = true
    const baselineRevision = git(['rev-parse', 'HEAD'], baselineRoot).trim()
    const currentRevision = git(['rev-parse', 'HEAD'], projectRoot).trim()
    const currentChanges = fingerprintChanges(projectRoot)

    const currentBench = prepareBench('current', projectRoot, scratch, projectRoot)
    const baselineBench = prepareBench('baseline', baselineRoot, scratch, projectRoot)
    const currentBinary = buildBench(currentBench, join(scratch, 'target-current'), projectRoot)
    const baselineBinary = buildBench(baselineBench, join(scratch, 'target-baseline'), projectRoot)

    const samples = {current: [], baseline: []}
    for (let sample = 0; sample < options.samples; sample += 1) {
        const order = sample % 2 === 0
            ? [['current', currentBinary], ['baseline', baselineBinary]]
            : [['baseline', baselineBinary], ['current', currentBinary]]
        for (const [name, binary] of order) {
            process.stderr.write(`sample ${sample + 1}/${options.samples} ${name}... `)
            const output = execBench(binary, roots, projectRoot, sample > 0)
            const parsed = parseSnapshot(output)
            // Exact per-file facts are needed only for the first paired
            // sample. Later samples prove determinism through the language
            // digest and contribute timings; retaining millions of duplicate
            // fact objects can exhaust the orchestrator before sample five.
            if (sample > 0) parsed.files.clear()
            samples[name].push(parsed)
            process.stderr.write('done\n')
        }
    }
    assertDeterministicFacts(samples.current, 'current')
    assertDeterministicFacts(samples.baseline, 'baseline')

    const facts = compareFacts(samples.baseline[0], samples.current[0])
    const throughput = compareThroughput(
        samples,
        options.maxSlowdownPct,
        options.minRegressionMs * 1_000_000,
    )
    const report = {
        schema: 'weavatrix.parse-revision-comparison.v1',
        generatedAt: new Date().toISOString(),
        corpus: {
            manifest: 'tools/real-corpus.json',
            repositories: corpusEntries,
            repositoryCount: roots.length,
            capBytesPerLanguage: manifest.capBytesPerLanguage,
            immutableSnapshot: {
                files: snapshot.files,
                bytes: snapshot.bytes,
                sha256: snapshot.sha256,
            },
        },
        revisions: {
            baseline: {requested: options.baseline, commit: baselineRevision},
            current: {
                commit: currentRevision,
                worktree: currentChanges.files.length ? 'DIRTY' : 'CLEAN',
                changedFiles: currentChanges.files,
                changedContentSha256: currentChanges.sha256,
            },
        },
        method: {
            sameHarnessSource: true,
            isolatedBaselineWorktree: true,
            alternatingProcessOrder: true,
            processSamples: options.samples,
            internalRounds: samples.current[0].rounds,
            throughputStatistic: 'median of per-process fastest complete extraction',
            maxSlowdownPct: options.maxSlowdownPct,
            minRegressionMs: options.minRegressionMs,
            throughputGate: 'both the relative and absolute slowdown thresholds must be exceeded',
            facts: 'exact stable-API declarations/imports/references counts and FNV-1a content digests per file',
            expectedFactsChange: 'Go may only reclassify grouped const/var names from references to declarations; imports must stay identical and declaration gains must equal reference removals.',
        },
        facts,
        throughput,
        gates: {
            factRegressions: facts.languages.filter((item) => item.state === 'REGRESSION').length,
            throughputRegressions: throughput.languages.filter((item) => item.state === 'REGRESSION').length,
        },
    }
    report.gates.pass = report.gates.factRegressions === 0
        && report.gates.throughputRegressions === 0
    writeFileSync(resolve(options.out), `${JSON.stringify(report, null, 2)}\n`)
    console.log(`wrote ${resolve(options.out)} pass=${report.gates.pass}`)
    if (!report.gates.pass) process.exitCode = 1
} finally {
    if (worktreeAdded) {
        try {
            git(['worktree', 'remove', '--force', baselineRoot], projectRoot)
        } catch (error) {
            process.stderr.write(`warning: could not remove baseline worktree: ${error.message}\n`)
        }
    }
    const resolvedScratch = resolve(scratch)
    const resolvedTemp = resolve(tmpdir())
    if (dirname(resolvedScratch) === resolvedTemp
        && basename(resolvedScratch).startsWith('weavatrix-parse-regression-')) {
        rmSync(resolvedScratch, {recursive: true, force: true})
    } else {
        throw new Error(`refusing to remove unexpected scratch directory: ${resolvedScratch}`)
    }
}
