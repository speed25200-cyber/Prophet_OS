// Régression du protocole : cardinalité des appels et fin de génération.
// Exercé contre le parseur et la grammaire du moteur, sans modèle ni réponse simulée.
#include "chat-auto-parser.h"
#include "chat.h"
#include "llama-grammar.h"
#include "peg-parser.h"

#include <fstream>
#include <iostream>
#include <iterator>
#include <stdexcept>

static bool grammar_accepts(const std::string & rules, const std::string & text) {
    auto * grammar = llama_grammar_init_impl(nullptr, rules.c_str(), "root", false, nullptr, 0, nullptr, 0);
    if (!grammar) throw std::runtime_error("grammaire invalide");
    bool accepted = true;
    for (const char c : text) {
        try {
            llama_grammar_accept_token(*grammar, 0, std::string(1, c));
        } catch (const std::runtime_error &) {
            accepted = false;
            break;
        }
        if (llama_grammar_get_stacks(grammar).empty()) {
            accepted = false;
            break;
        }
    }
    bool complete = false;
    if (accepted) {
        for (const auto & stack : llama_grammar_get_stacks(grammar)) complete |= stack.empty();
    }
    llama_grammar_free_impl(grammar);
    return accepted && complete;
}

int main(int argc, char ** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    const std::string source((std::istreambuf_iterator<char>(input)), {});
    common_chat_template tmpl(source, "", "");
    autoparser::generation_params params;
    params.messages = common_json::parse(R"([{"role":"user","content":"Prepare a note."}])");
    params.tools = common_json::parse(R"([
      {"type":"function","function":{"name":"fs.read","parameters":{"type":"object","properties":{"path":{"type":"string"},"max_bytes":{"type":"integer","minimum":0}},"required":["path"],"additionalProperties":false}}},
      {"type":"function","function":{"name":"fs.write","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}}}
    ])");
    params.reasoning_format = COMMON_REASONING_FORMAT_DEEPSEEK;
    params.enable_thinking = false;
    const std::string call = "<tool_call>\n{\"name\":\"fs.write\",\"arguments\":{\"path\":\"~/docs/note.txt\",\"content\":\"exact\\ntext\"}}\n</tool_call>";
    int failures = 0;
    auto expect = [&](bool ok, const std::string & label) {
        std::cout << (ok ? "OK " : "ECHEC ") << label << '\n';
        failures += !ok;
    };
    for (const bool parallel : {false, true}) {
        for (const auto choice : {COMMON_CHAT_TOOL_CHOICE_AUTO, COMMON_CHAT_TOOL_CHOICE_REQUIRED}) {
            params.parallel_tool_calls = parallel;
            params.tool_choice = choice;
            const auto data = autoparser::peg_generator::generate_parser(tmpl, params);
            const auto label = std::string(parallel ? "parallel " : "single ") + (choice == COMMON_CHAT_TOOL_CHOICE_AUTO ? "auto " : "required ");
            auto accepts = [&](const std::string & text) {
                return grammar_accepts(data.grammar, data.grammar_lazy ? text : data.generation_prompt + text);
            };
            expect(accepts(call), label + "one complete call");
            expect(!accepts(call.substr(0, call.size()-1)), label + "truncated call rejected");
            expect(accepts(call + "\n" + call) == parallel, label + "call cardinality");
            expect(!accepts(call + std::string(256, '\n')), label + "unbounded whitespace rejected");
            expect(!accepts("\n\n"), label + "no empty tool sequence");
            common_peg_arena parser;
            parser.load(data.parser);
            auto parses = [&](const std::string & text) {
                const auto prefixed = data.generation_prompt + text;
                common_peg_parse_context context(prefixed);
                return parser.parse(context).success();
            };
            expect(parses(call), label + "parser accepts call");
            expect(parses(call + "\n" + call) == parallel, label + "parser cardinality");
            if (choice == COMMON_CHAT_TOOL_CHOICE_AUTO) {
                common_chat_parser_params parse_params(data);
                parse_params.parser = parser;
                expect(common_chat_parse("Done.", false, parse_params).content == "Done.", label + "final answer contract");
            }
        }
    }
    return failures ? 1 : 0;
}
