// External scanner for the Jet tree-sitter grammar: block comments nest the
// way the Jet lexer reads them (S5), so `/* a /* b */ c */` is one comment.
// A regular expression cannot count depth, so this token is scanned here.

#include "tree_sitter/parser.h"

enum TokenType {
  BLOCK_COMMENT,
  STRING_GUARD,
};

void *tree_sitter_jet_external_scanner_create(void) { return NULL; }

void tree_sitter_jet_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_jet_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_jet_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static bool jet_is_space(int32_t c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

bool tree_sitter_jet_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  (void)payload;
  // Inside quoted text `/*` is text, never a comment: the guard symbol is
  // valid exactly where a string's content may continue.
  if (valid_symbols[STRING_GUARD] || !valid_symbols[BLOCK_COMMENT]) return false;
  while (jet_is_space(lexer->lookahead)) lexer->advance(lexer, true);
  if (lexer->lookahead != '/') return false;
  lexer->advance(lexer, false);
  if (lexer->lookahead != '*') return false;
  lexer->advance(lexer, false);
  unsigned depth = 1;
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '/') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '*') {
        lexer->advance(lexer, false);
        depth += 1;
      }
    } else if (lexer->lookahead == '*') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '/') {
        lexer->advance(lexer, false);
        depth -= 1;
        if (depth == 0) {
          lexer->mark_end(lexer);
          lexer->result_symbol = BLOCK_COMMENT;
          return true;
        }
      }
    } else {
      lexer->advance(lexer, false);
    }
  }
  // An unclosed comment is no token here; the compiler reports it.
  return false;
}
