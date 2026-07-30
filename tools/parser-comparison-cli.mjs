export function parseArgs(args) {
    const options = {
        baseline: 'HEAD',
        manifest: 'tools/real-corpus.json',
        out: null,
        samples: 3,
        maxSlowdownPct: 10,
        minRegressionMs: 1,
    }
    for (let index = 0; index < args.length; index += 1) {
        const argument = args[index]
        if (argument === '--baseline') options.baseline = args[++index]
        else if (argument === '--manifest') options.manifest = args[++index]
        else if (argument === '--out') options.out = args[++index]
        else if (argument === '--samples') options.samples = Number(args[++index])
        else if (argument === '--max-slowdown-pct') options.maxSlowdownPct = Number(args[++index])
        else if (argument === '--min-regression-ms') options.minRegressionMs = Number(args[++index])
        else throw new Error(`unknown argument: ${argument}`)
    }
    if (!Number.isInteger(options.samples) || options.samples < 1 || options.samples > 9) {
        throw new Error('--samples must be an integer from 1 to 9')
    }
    if (!Number.isFinite(options.maxSlowdownPct) || options.maxSlowdownPct < 0) {
        throw new Error('--max-slowdown-pct must be a non-negative number')
    }
    if (!Number.isFinite(options.minRegressionMs) || options.minRegressionMs < 0) {
        throw new Error('--min-regression-ms must be a non-negative number')
    }
    return options
}
