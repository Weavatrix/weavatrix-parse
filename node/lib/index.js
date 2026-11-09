'use strict'

const native = require('../index.js')

function extract(source, language) {
  return JSON.parse(native.extractFactsJson(source, language))
}

function extractPath(path, source) {
  const encoded = native.extractPathJson(path, source)
  return encoded == null ? undefined : JSON.parse(encoded)
}

function tokenize(source, language, options = {}) {
  return JSON.parse(native.tokenizeJson(source, language, options.mode === 'lite'))
}

module.exports = {
  extract,
  extractPath,
  tokenize,
  supportedLanguages: native.supportedLanguages,
}
