export function parseSnapshot(output) {
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
            addFactDetail(file, fields)
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

function addFactDetail(file, fields) {
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
}

export function assertDeterministicFacts(samples, name) {
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

export function compareFacts(baseline, current) {
    const languages = [...new Set([
        ...baseline.languages.keys(),
        ...current.languages.keys(),
    ])].sort().map((language) => compareLanguageFacts(language, baseline, current))
    return {
        exactUnchangedLanguages: languages.filter((item) => item.state === 'SAME').length,
        expectedChangedLanguages: languages
            .filter((item) => item.state === 'EXPECTED_GO_DECLARATION_RECLASSIFICATION').length,
        regressions: languages.filter((item) => item.state === 'REGRESSION').length,
        languages,
    }
}

function compareLanguageFacts(language, baseline, current) {
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

export function compareThroughput(samples, maxSlowdownPct, minRegressionNs) {
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
        const slowdownNs = currentMedian - baselineMedian
        return {
            language,
            state: slowdownPct !== null
                && slowdownPct > maxSlowdownPct
                && slowdownNs > minRegressionNs
                ? 'REGRESSION'
                : 'PASS',
            baselineNs,
            currentNs,
            baselineMedianNs: baselineMedian,
            currentMedianNs: currentMedian,
            slowdownNs,
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
