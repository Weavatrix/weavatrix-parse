'use strict'

const suffixes = {
  'win32-x64': 'win32-x64-msvc',
  'win32-arm64': 'win32-arm64-msvc',
  'darwin-x64': 'darwin-x64',
  'darwin-arm64': 'darwin-arm64',
  'linux-x64': 'linux-x64-gnu',
  'linux-arm64': 'linux-arm64-gnu',
}

const key = `${process.platform}-${process.arch}`
const suffix = suffixes[key]
if (!suffix) {
  throw new Error(`weavatrix-parse does not support ${key}`)
}

try {
  module.exports = require(`./weavatrix-parse.${suffix}.node`)
} catch (error) {
  const wrapped = new Error(`weavatrix-parse native binding is missing for ${key}; reinstall the package`)
  wrapped.cause = error
  throw wrapped
}
