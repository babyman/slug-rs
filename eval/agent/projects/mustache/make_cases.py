"""Writes cases.json for the Mustache project: the official specification's
tests for the modules spec.md requires (vendored in spec/), less the four
inheritance tests of block reindentation, which spec.md does not ask for,
then cases for what spec.md adds: Slug's data, syntax errors, `parse`, and
size.

Every `render` case renders its template from text and from `parse`'s result.
Expected output is written here from spec.md, not taken from the reference.
"""

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

MODULES = ["interpolation", "sections", "inverted", "comments", "partials", "delimiters", "~inheritance"]

# Block reindentation (new in spec 1.4) is left out of spec.md.
SKIPPED = {
    ("~inheritance", "Standalone block"),
    ("~inheritance", "Block reindentation"),
    ("~inheritance", "Intrinsic indentation"),
    ("~inheritance", "Nested block reindentation"),
}


def slug_id(module, name):
    text = (module.lstrip("~") + "-" + name).lower()
    return "".join(c if c.isalnum() else "-" for c in text).strip("-").replace("--", "-").replace("--", "-")


def render(id, template, data, expect, partials=None, seconds=None):
    case = {
        "id": id,
        "kind": "render",
        "template": template,
        "data": json.dumps(data, ensure_ascii=False),
        "partials": json.dumps(partials or {}, ensure_ascii=False),
        "expect": expect,
    }
    if seconds:
        case["seconds"] = seconds
    return case


def syntax_error(id, template, line, partials=None, data=None):
    case = {"id": id, "kind": "syntax_error", "template": template, "line": line}
    if partials:
        case["partials"] = json.dumps(partials, ensure_ascii=False)
        case["data"] = json.dumps(data or {}, ensure_ascii=False)
    return case


def spec_cases():
    cases = []
    for module in MODULES:
        for test in json.loads((HERE / "spec" / f"{module}.json").read_text())["tests"]:
            if (module, test["name"]) in SKIPPED:
                continue
            id = "spec-" + slug_id(module, test["name"])
            # The specification repeats a few names.
            if any(case["id"] == id for case in cases):
                id += "-2"
            cases.append(render(
                id,
                test["template"],
                test["data"],
                test["expected"],
                test.get("partials"),
            ))
    return cases


def data_cases():
    return [
        render("data-empty-string-is-truthy", "{{#s}}[{{.}}]{{/s}}{{^s}}no{{/s}}", {"s": ""}, "[]"),
        render("data-zero-is-truthy", "{{#n}}n={{.}}{{/n}}{{^n}}no{{/n}}", {"n": 0}, "n=0"),
        render("data-empty-map-is-truthy", "{{#m}}yes{{/m}}{{^m}}no{{/m}}", {"m": {}}, "yes"),
        render("data-empty-list-is-falsy", "{{#l}}yes{{/l}}{{^l}}no{{/l}}", {"l": []}, "no"),
        render("data-nil-key-hides-outer", "{{#inner}}[{{a}}]{{/inner}}", {"a": "outer", "inner": {"a": None}}, "[]"),
        render("data-missing-key-reaches-outer", "{{#inner}}[{{a}}]{{/inner}}", {"a": "outer", "inner": {}}, "[outer]"),
        render("data-booleans-interpolate", "{{t}} {{f}}", {"t": True, "f": False}, "true false"),
        render("data-numbers-interpolate", "{{x}} {{y}} {{z}}", {"x": -2.5, "y": 100, "z": 0}, "-2.5 100 0"),
        render("data-escapes-four-characters", "{{s}}", {"s": "<a href=\"x\">'&'</a>"},
               "&lt;a href=&quot;x&quot;&gt;'&amp;'&lt;/a&gt;"),
        render("data-raw-forms-do-not-escape", "{{{s}}} {{&s}}", {"s": "<b>&\"</b>"}, "<b>&\"</b> <b>&\"</b>"),
        render("data-no-list-indexes", "[{{a.0}}]", {"a": ["x"]}, "[]"),
        render("data-dotted-through-string", "[{{a.b}}]", {"a": "text"}, "[]"),
        render("data-nested-lists-see-outer-names",
               "{{#rows}}{{#cells}}{{.}}{{sep}}{{/cells}};{{/rows}}",
               {"rows": [{"cells": [1, 2]}, {"cells": [3]}], "sep": "|"}, "1|2|;3|;"),
        render("data-section-over-scalar-pushes-it", "{{#name}}<{{.}}>{{/name}}", {"name": "Ada"}, "<Ada>"),
        render("data-root-string", "{{.}}!", "hi", "hi!"),
        render("data-root-nil", "[{{.}}][{{a}}]{{^.}}empty{{/.}}", None, "[][]empty"),
        render("data-unicode", "{{s}}é", {"s": "café ☕"}, "café ☕é"),
        render("data-list-of-maps-and-scalars", "{{#xs}}({{n}}){{/xs}}", {"xs": [{"n": 1}, "s", 2], "n": "N"},
               "(1)(N)(N)"),
    ]


def template_cases():
    return [
        render("tags-comment-ends-at-first-closing", "a{{! {{b}} }}c", {}, "a }}c"),
        render("tags-delimiters-outlast-section", "{{#a}}{{=<% %>=}}x<%/a%><%b%>{{b}}", {"a": True, "b": "B"},
               "xB{{b}}"),
        render("tags-triple-with-set-delimiters", "{{=<% %>=}}<%{s}%>|<%&s%>|<%s%>", {"s": "<b>"},
               "<b>|<b>|&lt;b&gt;"),
        render("tags-set-delimiters-padded", "{{=   [[   ]]   =}}[[ a ]]", {"a": 1}, "1"),
        render("tags-padded-sigils", "{{# a }}{{ b }}{{/ a }}{{& c }}{{{ c }}}", {"a": True, "b": "x", "c": "<"},
               "x<<"),
        render("tags-unknown-sigil-is-a-name", "{{%x}}{{?y}}", {"%x": 1, "?y": 2}, "12"),
        render("standalone-two-section-tags", "a\n  {{#t}}{{/t}}  \nb\n", {"t": True}, "a\nb\n"),
        render("standalone-tag-and-text-is-not", "a\n{{#t}}x{{/t}}\nb", {"t": True}, "a\nx\nb"),
        render("standalone-interpolation-is-not", "a\n  {{#t}}{{v}}{{/t}}\nb", {"t": True, "v": ""}, "a\n  \nb"),
        render("standalone-lone-cr-is-not-a-line-end", "a\r{{#t}}\rb{{/t}}", {"t": True}, "a\r\rb"),
        render("partials-default-delimiters", "{{=<% %>=}}<%#o%><%>p%><%/o%>", {"o": {"v": "in"}}, "in",
               {"p": "{{v}}"}),
        render("partials-indent-blank-lines", "  {{>p}}\n", {}, "  a\n  \n  b\n", {"p": "a\n\nb\n"}),
        render("partials-indent-without-final-newline", "x\n  {{>p}}\ny", {}, "x\n  a\n  by", {"p": "a\nb"}),
        render("partials-missing-standalone", "x\n  {{>nope}}\ny", {}, "x\ny"),
        render("partials-inline-not-indented", "x {{>p}}\n", {}, "x a\nb\n\n", {"p": "a\nb\n"}),
        render("partials-see-section-context", "{{#people}}{{>row}}{{/people}}",
               {"people": [{"name": "A"}, {"name": "B"}]}, "<A><B>", {"row": "<{{name}}>"}),
        render("partials-unused-broken-partial", "{{#no}}{{>p}}{{/no}}ok", {}, "ok", {"p": "{{#x}}"}),
        render("inheritance-block-at-top-level", "{{$t}}default {{v}}{{/t}}", {"v": 1}, "default 1"),
        render("inheritance-missing-parent", "[{{<nope}}{{$a}}x{{/a}}{{/nope}}]", {}, "[]"),
        render("inheritance-parent-sees-section-context", "{{#user}}{{<card}}{{/card}}{{/user}}", {"user": {"name": "Ada"}},
               "card: Ada", {"card": "card: {{name}}"}),
        render("inheritance-arguments-reach-partials", "{{<layout}}{{$title}}Home{{/title}}{{/layout}}", {},
               "<h1>Home</h1>", {"layout": "{{>header}}", "header": "<h1>{{$title}}Untitled{{/title}}</h1>"}),
        render("inheritance-nested-sections-inside-parent-ignored", "{{<p}}{{#x}}{{$a}}no{{/a}}{{/x}}{{/p}}", {"x": True},
               "A", {"p": "{{$a}}A{{/a}}"}),
    ]


def error_cases():
    return [
        syntax_error("error-unclosed-tag", "Hello {{name", 1),
        syntax_error("error-unclosed-triple", "x\n{{{a}}", 2),
        syntax_error("error-unclosed-comment", "a\nb\n{{! never closed", 3),
        syntax_error("error-unclosed-section", "a\nb {{#s}}\nc", 2),
        syntax_error("error-unclosed-inverted", "{{^s}}\n\n", 1),
        syntax_error("error-unclosed-parent", "x\n{{<p}}", 2),
        syntax_error("error-mismatched-end", "{{#a}}\n{{/b}}", 2),
        syntax_error("error-end-without-section", "x\n\n{{/a}}", 3),
        syntax_error("error-unclosed-block-in-parent", "{{<p}}\n{{$b}}x\n{{/p}}", 3),
        syntax_error("error-one-delimiter", "a\n{{=<% =}}", 2),
        syntax_error("error-three-delimiters", "{{=<% %> x=}}", 1),
        syntax_error("error-space-in-name", "a\n{{first name}}", 2),
        syntax_error("error-empty-partial-name", "a\n{{>}}", 2),
        syntax_error("error-empty-variable", "a {{ }} b", 1),
        syntax_error("error-unrendered-section", "{{#never}}x", 1),
        syntax_error("error-after-set-delimiters", "{{=<% %>=}}\n<%#a%>\n{{/a}}", 2),
        syntax_error("error-in-used-partial", "ok {{>p}}", 2, {"p": "a\n{{#x}}"}),
        syntax_error("error-in-indented-partial", "ok\n  {{>p}}\n", 3, {"p": "a\nb\n{{/x}}\n"}),
    ]


PRELUDE = 'val mustache = import("{module}")\n'


def program(id, source):
    return {"id": id, "kind": "program", "source": source}


def program_cases():
    return [
        program("parse-result-renders-with-new-data", """\
val template = mustache.parse("{{#items}}{{.}},{{/items}}")
val a = mustache.render(template, {items: [1, 2]})
val b = mustache.render(template, {items: ["x"]})
if (a == "1,2," && b == "x,") { println("PASS") } else { println("FAIL", a, b) }
"""),
        program("partials-resolve-at-render", """\
val template = mustache.parse("[{{>p}}]")
val a = mustache.render(template, {}, {p: "x"})
val b = mustache.render(template, {}, {p: "y"})
val c = mustache.render(template, {})
if (a == "[x]" && b == "[y]" && c == "[]") { println("PASS") } else { println("FAIL", a, b, c) }
"""),
        program("slug-values-as-data", """\
val data = {title: "Pack", items: [{letter: "a", name: "mace", worn: false}, {letter: "b", name: "mail", worn: true}]}
val out = mustache.render("{{title}}:{{#items}} {{letter}}){{name}}{{#worn}}*{{/worn}}{{/items}}", data)
if (out == "Pack: a)mace b)mail*") { println("PASS") } else { println("FAIL", out) }
"""),
        program("partial-errors-surface-at-render", """\
val template = mustache.parse("{{>p}}")
val failed = fn() {
  defer onerror(e) { return e.type == "mustache.syntax" && e.data.line == 1 }
  mustache.render(template, {}, {p: "{{#x}}"})
  false
}
if (failed()) { println("PASS") } else { println("FAIL no error") }
"""),
    ]


def size_cases():
    items = [{"name": f"item {i} <{i % 7}>", "n": i} for i in range(20000)]
    expect = "".join(f"<li>{item['name'].replace('<', '&lt;').replace('>', '&gt;')} {item['n']}</li>\n" for item in items)
    long_template = "Hello {{name}}! {{#show}}shown{{/show}}\n" * 5000
    long_expect = "Hello Ada! shown\n" * 5000
    return [
        render("size-twenty-thousand-items", "{{#items}}\n<li>{{name}} {{n}}</li>\n{{/items}}\n", {"items": items}, expect,
               seconds=20),
        render("size-long-template", long_template, {"name": "Ada", "show": True}, long_expect, seconds=20),
    ]


def main():
    cases = spec_cases() + data_cases() + template_cases() + error_cases() + program_cases() + size_cases()
    ids = [case["id"] for case in cases]
    assert len(ids) == len(set(ids)), "duplicate case ids"
    suite = {"prelude": PRELUDE, "cases": cases}
    (HERE / "cases.json").write_text(json.dumps(suite, indent=1, ensure_ascii=False) + "\n")
    print(f"{len(cases)} cases")


if __name__ == "__main__":
    main()
