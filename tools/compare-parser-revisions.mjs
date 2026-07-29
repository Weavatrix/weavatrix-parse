// Reproducible current-vs-baseline parser gate.
//
// The same regression-bench source is compiled against both parser trees.
// Corpus paths come from tools/real-corpus.json and remain relative in Git.
// A detached baseline worktree and both Cargo target directories live under
// one verified OS-temp directory and are removed on exit.
//
//   node tools/compare-parser-revisions.mjs \
//     --baseline HEAD --samples 3 --max-slowdown-pct 10 \
//     --out target/parser-regression.json
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
    cpSync,
    existsSync,
    mkdirSync,
    mkdtempSync,
    readdirSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

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
    const snapshot = snapshotCorpus(sourceRoots, join(scratch, 'corpus'))
    const roots = snapshot.roots
    git(['worktree', 'add', '--detach', baselineRoot, options.baseline], projectRoot)
    worktreeAdded = true
    const baselineRevision = git(['rev-parse', 'HEAD'], baselineRoot).trim()
    const currentRevision = git(['rev-parse', 'HEAD'], projectRoot).trim()
    const currentChanges = fingerprintChanges(projectRoot)

    const currentBench = prepareBench('current', projectRoot)
    const baselineBench = prepareBench('baseline', baselineRoot)
    const currentBinary = buildBench(currentBench, join(scratch, 'target-current'))
    const baselineBinary = buildBench(baselineBench, join(scratch, 'target-baseline'))

    const samples = {current: [], baseline: []}
    for (let sample = 0; sample < options.samples; sample += 1) {
        const order = sample % 2 === 0
            ? [['current', currentBinary], ['baseline', baselineBinary]]
            : [['baseline', baselineBinary], ['current', currentBinary]]
        for (const [name, binary] of order) {
            process.stderr.write(`sample ${sample + 1}/${options.samples} ${name}... `)
            const output = exec(binary, roots, projectRoot)
            const parsed = parseSnapshot(output)
            samples[name].push(parsed)
            process.stderr.write('done\n')
        }
    }
    assertDeterministicFacts(samples.current, 'current')
    assertDeterministicFacts(samples.baseline, 'baseline')

    const facts = compareFacts(samples.baseline[0], samples.current[0])
    const throughput = compareThroughput(samples, options.maxSlowdownPct)
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

function prepareBench(name, parserRoot) {
    const destination = join(scratch, `bench-${name}`)
    cpSync(join(projectRoot, 'tools', 'regression-bench'), destination, {recursive: true})
    const manifestFile = join(destination, 'Cargo.toml')
    const parserPath = resolve(parserRoot).replace(/\\/g, '/')
    const cargo = readFileSync(manifestFile, 'utf8')
        .replace('path = "../.."', `path = ${JSON.stringify(parserPath)}`)
    writeFileSync(manifestFile, cargo)
    return destination
}

function buildBench(benchRoot, targetDir) {
    const manifest = join(benchRoot, 'Cargo.toml')
    execFileSync('cargo', ['build', '--release', '--manifest-path', manifest], {
        cwd: projectRoot,
        env: {...process.env, CARGO_TARGET_DIR: targetDir},
        stdio: 'inherit',
        timeout: 20 * 60_000,
        windowsHide: true,
    })
    return join(
        targetDir,
        'release',
        process.platform === 'win32'
            ? 'weavatrix-parse-regression-bench.exe'
            : 'weavatrix-parse-regression-bench',
    )
}

function parseSnapshot(output) {
    const lines = output.replace(/^\uFEFF/, '').split(/\r?\n/).filter(Boolean)
    const header = Object.fromEntries(lines[0].split(/\s+/).map((part) => part.split('=')))
    if (header.schema !== 'weavatrix.parse-regression.v1') {
        throw new Error(`unexpected regression output: ${lines[0]}`)
    }
    const files = new Map()
    const languages = new Map()
    for (const line of lines.slice(1)) {
        const fields = line.split('\t')
        if (fields[0] === 'F') {
            const item = {
                language: fields[1],
                identity: unescapeField(fields[2]),
                bytes: Number(fields[3]),
                declarations: Number(fields[4]),
                imports: Number(fields[5]),
                references: Number(fields[6]),
                digest: fields[7],
                declarationFacts: [],
                importFacts: [],
                referenceFacts: [],
            }
            files.set(`${item.language}\0${item.identity}`, item)
        } else if (fields[0] === 'D' || fields[0] === 'I' || fields[0] === 'R') {
            const identity = unescapeField(fields[2])
            const file = files.get(`${fields[1]}\0${identity}`)
            if (!file) throw new Error(`fact detail preceded file record: ${line}`)
            if (fields[0] === 'D') {
                file.declarationFacts.push({
                    name: unescapeField(fields[3]),
                    kind: fields[4],
                    start: Number(fields[5]),
                    end: Number(fields[6]),
                    line: Number(fields[7]),
                    owner: unescapeField(fields[8]) || null,
                })
            } else if (fields[0] === 'I') {
                file.importFacts.push({
                    specifier: unescapeField(fields[3]),
                    reexport: fields[4] === 'true',
                    start: Number(fields[5]),
                    end: Number(fields[6]),
                    line: Number(fields[7]),
                })
            } else {
                file.referenceFacts.push({
                    name: unescapeField(fields[3]),
                    kind: fields[4],
                    start: Number(fields[5]),
                    end: Number(fields[6]),
                    line: Number(fields[7]),
                    owner: unescapeField(fields[8]) || null,
                })
            }
        } else if (fields[0] === 'L') {
            languages.set(fields[1], {
                language: fields[1],
                files: Number(fields[2]),
                bytes: Number(fields[3]),
                declarations: Number(fields[4]),
                imports: Number(fields[5]),
                references: Number(fields[6]),
                digest: fields[7],
                elapsedNs: Number(fields[8]),
            })
        }
    }
    return {rounds: Number(header.rounds), cap: Number(header.cap), files, languages}
}

function assertDeterministicFacts(samples, name) {
    const first = samples[0]
    for (const [sampleIndex, sample] of samples.entries()) {
        for (const [language, expected] of first.languages) {
            const actual = sample.languages.get(language)
            if (!actual || actual.digest !== expected.digest) {
                throw new Error(`${name} facts changed between process samples for ${language} at sample ${sampleIndex + 1}`)
            }
        }
    }
}

function compareFacts(baseline, current) {
    const languages = [...new Set([
        ...baseline.languages.keys(),
        ...current.languages.keys(),
    ])].sort().map((language) => {
        const before = baseline.languages.get(language)
        const after = current.languages.get(language)
        const changedFiles = compareFiles(language, baseline.files, current.files)
        let state = 'SAME'
        let reason = null
        if (!before || !after) {
            state = 'REGRESSION'
            reason = 'language disappeared from one revision'
        } else if (before.digest !== after.digest) {
            const expectedGoReclassification = language === 'go'
                && changedFiles.length > 0
                && changedFiles.every((change) =>
                    change.baseline
                    && change.current
                    && change.current.declarations > change.baseline.declarations
                    && change.current.imports === change.baseline.imports
                    && change.current.declarations - change.baseline.declarations
                        === change.baseline.references - change.current.references)
            state = expectedGoReclassification
                ? 'EXPECTED_GO_DECLARATION_RECLASSIFICATION'
                : 'REGRESSION'
            reason = expectedGoReclassification
                ? 'grouped Go names moved one-for-one from references to declarations'
                : 'stable facts changed outside the allowed grouped-Go declaration shape'
        }
        return {
            language,
            state,
            reason,
            baseline: before || null,
            current: after || null,
            changedFileCount: changedFiles.length,
            changedFiles: changedFiles.slice(0, 40),
        }
    })
    return {
        exactUnchangedLanguages: languages.filter((item) => item.state === 'SAME').length,
        expectedChangedLanguages: languages
            .filter((item) => item.state === 'EXPECTED_GO_DECLARATION_RECLASSIFICATION').length,
        regressions: languages.filter((item) => item.state === 'REGRESSION').length,
        languages,
    }
}

function compareFiles(language, baseline, current) {
    const keys = new Set([
        ...[...baseline.keys()].filter((key) => key.startsWith(`${language}\0`)),
        ...[...current.keys()].filter((key) => key.startsWith(`${language}\0`)),
    ])
    return [...keys].sort().flatMap((key) => {
        const before = baseline.get(key)
        const after = current.get(key)
        if (before?.digest === after?.digest) return []
        return [{
            identity: (after || before).identity,
            baseline: withoutFactDetails(before),
            current: withoutFactDetails(after),
            factDeltas: {
                declarations: factDelta(before?.declarationFacts, after?.declarationFacts),
                imports: factDelta(before?.importFacts, after?.importFacts),
                references: factDelta(before?.referenceFacts, after?.referenceFacts),
            },
        }]
    })
}

function withoutFactDetails(file) {
    if (!file) return null
    const {
        declarationFacts: _declarationFacts,
        importFacts: _importFacts,
        referenceFacts: _referenceFacts,
        ...summary
    } = file
    return summary
}

function factDelta(before = [], after = []) {
    const key = (item) => JSON.stringify(item)
    const beforeKeys = new Set(before.map(key))
    const afterKeys = new Set(after.map(key))
    return {
        added: after.filter((item) => !beforeKeys.has(key(item))).slice(0, 80),
        removed: before.filter((item) => !afterKeys.has(key(item))).slice(0, 80),
    }
}

function compareThroughput(samples, maxSlowdownPct) {
    const languageNames = [...new Set(samples.current.flatMap((sample) =>
        [...sample.languages.keys()]))].sort()
    const languages = languageNames.map((language) => {
        const currentNs = samples.current
            .map((sample) => sample.languages.get(language)?.elapsedNs)
            .filter(Number.isFinite)
        const baselineNs = samples.baseline
            .map((sample) => sample.languages.get(language)?.elapsedNs)
            .filter(Number.isFinite)
        const currentMedian = median(currentNs)
        const baselineMedian = median(baselineNs)
        const slowdownPct = baselineMedian > 0
            ? 100 * (currentMedian / baselineMedian - 1)
            : null
        return {
            language,
            state: slowdownPct !== null && slowdownPct > maxSlowdownPct
                ? 'REGRESSION'
                : 'PASS',
            baselineNs,
            currentNs,
            baselineMedianNs: baselineMedian,
            currentMedianNs: currentMedian,
            slowdownPct: slowdownPct === null ? null : round(slowdownPct),
            currentOverBaselineThroughput: currentMedian > 0
                ? round(baselineMedian / currentMedian)
                : null,
        }
    })
    return {
        regressions: languages.filter((item) => item.state === 'REGRESSION').length,
        languages,
    }
}

function exec(binary, roots, cwd) {
    return execFileSync(binary, roots, {
        cwd,
        encoding: 'utf8',
        maxBuffer: 512 * 1024 * 1024,
        timeout: 20 * 60_000,
        windowsHide: true,
    })
}

function git(args, cwd) {
    return execFileSync('git', args, {
        cwd,
        encoding: 'utf8',
        timeout: 2 * 60_000,
        windowsHide: true,
        stdio: ['ignore', 'pipe', 'pipe'],
    })
}

function fingerprintChanges(root) {
    const status = git([
        'status',
        '--porcelain=v1',
        '--untracked-files=all',
        '--',
        'Cargo.toml',
        'src',
    ], root).split(/\r?\n/).filter(Boolean)
    const hash = createHash('sha256')
    const files = status.map((line) => {
        const rawPath = line.slice(3)
        const path = rawPath.includes(' -> ') ? rawPath.split(' -> ').at(-1) : rawPath
        const absolute = resolve(root, path)
        hash.update(`${line}\0`)
        if (existsSync(absolute)) hash.update(readFileSync(absolute))
        else hash.update('<deleted>')
        hash.update('\0')
        return {status: line.slice(0, 2), path}
    })
    return {
        files,
        sha256: files.length ? hash.digest('hex') : null,
    }
}

function snapshotCorpus(sourceRoots, destination) {
    const cap = Number(manifest.capBytesPerLanguage)
    const totals = new Map()
    const hash = createHash('sha256')
    let files = 0
    let bytes = 0
    const roots = sourceRoots.map((_root, index) => {
        const path = join(destination, String(index))
        mkdirSync(path, {recursive: true})
        return path
    })
    for (const [rootIndex, sourceRoot] of sourceRoots.entries()) {
        visitSnapshot(sourceRoot, sourceRoot, roots[rootIndex], rootIndex)
    }
    return {roots, files, bytes, sha256: hash.digest('hex')}

    function visitSnapshot(sourceRoot, path, snapshotRoot, rootIndex) {
        let entries
        try {
            entries = readdirSync(path, {withFileTypes: true})
        } catch {
            return
        }
        entries.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0)
        for (const entry of entries) {
            if (entry.isSymbolicLink() || skipDirectory(entry.name)) continue
            const absolute = join(path, entry.name)
            if (entry.isDirectory()) {
                visitSnapshot(sourceRoot, absolute, snapshotRoot, rootIndex)
                continue
            }
            const language = languageFor(entry.name)
            if (!language || (totals.get(language) || 0) >= cap) continue
            let content
            try {
                content = readFileSync(absolute)
                new TextDecoder('utf-8', {fatal: true}).decode(content)
            } catch {
                continue
            }
            const relative = absolute.slice(sourceRoot.length).replace(/^[\\/]+/, '')
            const target = join(snapshotRoot, relative)
            mkdirSync(dirname(target), {recursive: true})
            writeFileSync(target, content)
            totals.set(language, (totals.get(language) || 0) + content.length)
            hash.update(`${rootIndex}/${relative.replace(/\\/g, '/')}\0`)
            hash.update(content)
            hash.update('\0')
            files += 1
            bytes += content.length
        }
    }
}

function languageFor(name) {
    const extension = name.includes('.') ? name.split('.').at(-1).toLowerCase() : ''
    return {
        js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript',
        ts: 'typescript', tsx: 'typescript', mts: 'typescript', cts: 'typescript',
        py: 'python', pyi: 'python', rs: 'rust', go: 'go', java: 'java',
        cs: 'csharp', c: 'c', h: 'c', cc: 'cpp', cpp: 'cpp', cxx: 'cpp',
        hpp: 'cpp', hxx: 'cpp', sql: 'sql', swift: 'swift', tf: 'terraform',
        hcl: 'terraform', xml: 'xml', md: 'markdown', markdown: 'markdown',
        sh: 'bash', bash: 'bash', zsh: 'bash',
    }[extension] || null
}

function skipDirectory(name) {
    return name.startsWith('.')
        || new Set([
            'node_modules',
            'target',
            'dist',
            'build',
            'coverage',
            'out',
            '__pycache__',
            'venv',
        ]).has(name)
}

function parseArgs(args) {
    const options = {
        baseline: 'HEAD',
        manifest: 'tools/real-corpus.json',
        out: null,
        samples: 3,
        maxSlowdownPct: 10,
    }
    for (let index = 0; index < args.length; index += 1) {
        const argument = args[index]
        if (argument === '--baseline') options.baseline = args[++index]
        else if (argument === '--manifest') options.manifest = args[++index]
        else if (argument === '--out') options.out = args[++index]
        else if (argument === '--samples') options.samples = Number(args[++index])
        else if (argument === '--max-slowdown-pct') options.maxSlowdownPct = Number(args[++index])
        else throw new Error(`unknown argument: ${argument}`)
    }
    if (!Number.isInteger(options.samples) || options.samples < 1 || options.samples > 9) {
        throw new Error('--samples must be an integer from 1 to 9')
    }
    if (!Number.isFinite(options.maxSlowdownPct) || options.maxSlowdownPct < 0) {
        throw new Error('--max-slowdown-pct must be a non-negative number')
    }
    return options
}

function median(values) {
    if (!values.length) return null
    const sorted = [...values].sort((left, right) => left - right)
    return sorted[Math.floor(sorted.length / 2)]
}

function round(value) {
    return Math.round(value * 100) / 100
}

function unescapeField(value) {
    return value.replace(/\\([\\trn])/g, (_match, escaped) => ({
        '\\': '\\',
        t: '\t',
        r: '\r',
        n: '\n',
    })[escaped])
}
