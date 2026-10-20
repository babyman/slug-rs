"""Writes the JSON project's hidden suite as Slug tests for `slug.fmt.json`,
the library chosen from the project's runs:

    python3 eval/agent/projects/json/make_slug_tests.py

Run it after changing cases.json. The tests go in the core clutch, which eval
installs leave out, since they hold the hidden cases.
"""

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
cases = json.loads((HERE / "cases.json").read_text())["cases"]
out = HERE.parents[3] / "clutch/slug.core.clutch/tests/json_suite.slug"

def lit(s):
    r = []
    for c in s:
        if c == "\\": r.append("\\\\")
        elif c == '"': r.append('\\"')
        elif c == "$": r.append("\\$")
        elif c == "\n": r.append("\\n")
        elif c == "\r": r.append("\\r")
        elif c == "\t": r.append("\\t")
        elif c == "\b": r.append("\\b")
        elif c == "\f": r.append("\\f")
        elif ord(c) < 0x20 or ord(c) == 0x7f: r.append("\\%03o" % ord(c))
        else: r.append(c)
    return '"' + "".join(r) + '"'

def block(rows):
    return ",\n".join(rows)

by = lambda kind: [c for c in cases if c["kind"] == kind and "generate" not in c]

parse = [f"    // {c['id']}\n    [{lit(c['input'])}], {c['expect']}" for c in by("parse")]
offsets = [f"    // {c['id']}\n    [{lit(c['input'])}], {c['offset']}" for c in by("parse_error") if "offset" in c]
unplaced = [f"    // {c['id']}\n    [{lit(c['input'])}], \"json.parse\"" for c in by("parse_error") if "offset" not in c]
roundtrips = [f"    // {c['id']}\n    [{lit(c['input'])}], {lit(c['expect'])}" for c in by("roundtrip")]
writes = [f"    // {c['id']}\n    [{c['value']}], {lit(c['expect'])}" for c in by("stringify")]
setups = sorted({c["setup"] for c in by("stringify_error") if c.get("setup")})
types = [f"    // {c['id']}\n    [{c['value']}], \"json.type\"" for c in by("stringify_error")]

text = f'''// The JSON project's hidden suite (eval/agent/projects/json/cases.json),
// written as Slug tests by eval/agent/projects/json/make_slug_tests.py; do not
// edit by hand. Each case's id is the comment above its example.

val json = import("slug.fmt.json")
val {{assertEqual, assertThrows}} = import("slug.test")

{chr(10).join(setups)}

@testWith(
{block(parse)}
)
val parses = fn(text:str) {{ json.parse(text) }}

// Where parsing `text` fails.
val failsAt = fn(text:str):num {{
    assertThrows(fn() {{ json.parse(text) }}, "json.parse").data.offset
}}

@testWith(
{block(offsets)}
)
val reportsTheOffset = failsAt

// Numbers too large for binary64 fail; the spec does not fix the offset.
val failsWith = fn(text:str):str {{
    assertThrows(fn() {{ json.parse(text) }}, "json.parse").type
}}

@testWith(
{block(unplaced)}
)
val rejects = failsWith

@testWith(
{block(roundtrips)}
)
val roundTrips = fn(text:str):str {{ json.stringify(json.parse(text)) }}

@testWith(
{block(writes)}
)
val writes = fn(value):str {{ json.stringify(value) }}

// What writing `value` throws.
val refuses = fn(value):str {{
    assertThrows(fn() {{ json.stringify(value) }}, "json.type").type
}}

@testWith(
{block(types)}
)
val rejectsValuesWithoutAJsonForm = refuses

// size-large-document: records like the suite's, written and read back. The
// suite times 4000; 1000 keep this check quick, since it is not timed here.
@test
val roundTripsALargeDocument = fn() {{
    val record = fn(index:num) {{
        val tag = index % 13
        {{
            id: index,
            name: "record $index café",
            note: if (index % 7 == 0) {{ "line\\nbreak\\t\\"quoted\\"" }} else {{ nil }},
            tags: ["a", "b", "$tag"],
            active: index % 2 == 0,
            nested: {{depth: [index, [index + 1, {{leaf: index % 5}}]]}},
        }}
    }}
    val records = fn(i:num, out:list):list {{ if (i == 1000) {{ out }} else {{ recur(i + 1, out :+ record(i)) }} }}
    val written = json.stringify(records(0, []))
    assertEqual(json.parse(written), records(0, []))
    assertEqual(json.stringify(json.parse(written)), written)
}}

// size-deep-nesting: arrays nested 1000 deep.
@test
val roundTripsDeepNesting = fn() {{
    val text = "[" * 1000 + "]" * 1000
    assertEqual(json.stringify(json.parse(text)), text)
}}
'''
out.write_text(text)
