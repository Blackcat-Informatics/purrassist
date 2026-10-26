// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
#include "llama-grammar.h"
#include <fstream>
#include <iterator>
#include <iostream>
int main(int argc, char **argv) {
 if(argc < 2) return 2;
 for(int i=1;i<argc;++i) {
  std::ifstream file(argv[i]);
  if(!file) return 2;
  std::string input((std::istreambuf_iterator<char>(file)), {});
  llama_grammar_parser parser;
  if(!parser.parse(input.c_str()) || !parser.symbol_ids.count("root")) return 1;
  std::cout << argv[i] << ": PASS\n";
 }
}
