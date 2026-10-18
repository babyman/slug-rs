"""A reference implementation of Tiny in Python, ported from
reference/tiny.slug so the two implementations agree on every detail of the
specification."""

import sys
import threading

KEYWORDS = {"let", "fn", "return", "if", "else", "while", "true", "false", "nil", "and", "or", "not", "print"}
MAX_INTEGER = 9223372036854775807
TWO = {"==", "!=", "<=", ">="}
ONE = set("+-*/%<>=(){}[],;")


class TinyError(Exception):
    def __init__(self, kind, line, column):
        super().__init__(f"{kind} error at {line}:{column}")
        self.kind = kind
        self.line = line
        self.column = column


class Token:
    def __init__(self, kind, line, column, value=None):
        self.kind = kind
        self.line = line
        self.column = column
        self.value = value


class Function:
    def __init__(self, name, params, body, scope):
        self.name = name
        self.params = params
        self.body = body
        self.scope = scope


class Builtin:
    def __init__(self, name):
        self.name = name


def is_digit(c):
    return "0" <= c <= "9"


def is_letter(c):
    return "a" <= c <= "z" or "A" <= c <= "Z" or c == "_"


def lex(source):
    tokens = []
    i, line, column, size = 0, 1, 1, len(source)
    at = lambda k: source[k] if k < size else ""
    while True:
        c = at(i)
        if c == "":
            tokens.append(Token("end", line, column))
            return tokens
        if c == "\n":
            i, line, column = i + 1, line + 1, 1
            continue
        if c in " \t\r":
            i, column = i + 1, column + 1
            continue
        if c == "#":
            k = i
            while at(k) not in ("", "\n"):
                k += 1
            i, column = k, column + (k - i)
            continue
        if is_digit(c):
            k, n = i, 0
            while is_digit(at(k)):
                n = n * 10 + ord(at(k)) - 48
                if n > MAX_INTEGER:
                    raise TinyError("syntax", line, column)
                k += 1
            tokens.append(Token("int", line, column, n))
            i, column = k, column + (k - i)
            continue
        if c == '"':
            k, out = i + 1, []
            while True:
                d = at(k)
                if d in ("", "\n"):
                    raise TinyError("syntax", line, column)
                if d == '"':
                    k += 1
                    break
                if d != "\\":
                    out.append(d)
                    k += 1
                    continue
                escaped = {"n": "\n", "t": "\t", '"': '"', "\\": "\\"}.get(at(k + 1))
                if escaped is None:
                    raise TinyError("syntax", line, column + (k - i))
                out.append(escaped)
                k += 2
            tokens.append(Token("string", line, column, "".join(out)))
            i, column = k, column + (k - i)
            continue
        if is_letter(c):
            k = i
            while is_letter(at(k)) or is_digit(at(k)):
                k += 1
            name = source[i:k]
            tokens.append(Token(name if name in KEYWORDS else "name", line, column, name))
            i, column = k, column + (k - i)
            continue
        pair = c + at(i + 1)
        if pair in TWO:
            tokens.append(Token(pair, line, column))
            i, column = i + 2, column + 2
            continue
        if c in ONE:
            tokens.append(Token(c, line, column))
            i, column = i + 1, column + 1
            continue
        raise TinyError("syntax", line, column)


# Statements and expressions are dicts with a `node` kind and the position
# their errors are reported at.
class Parser:
    LEVELS = [{"or"}, {"and"}, {"==", "!="}, {"<", "<=", ">", ">="}, {"+", "-"}, {"*", "/", "%"}]

    def __init__(self, tokens):
        self.tokens = tokens
        self.pos = 0
        self.depth = 0

    def peek(self):
        return self.tokens[self.pos]

    def ahead(self):
        return self.tokens[min(self.pos + 1, len(self.tokens) - 1)]

    def next(self):
        token = self.tokens[self.pos]
        self.pos += 1
        return token

    def unexpected(self):
        token = self.peek()
        raise TinyError("syntax", token.line, token.column)

    def expect(self, kind):
        if self.peek().kind != kind:
            self.unexpected()
        return self.next()

    @staticmethod
    def at(token, **fields):
        return {"line": token.line, "column": token.column, **fields}

    def params(self):
        self.expect("(")
        names = []
        if self.peek().kind == ")":
            self.next()
            return names
        while True:
            names.append(self.expect("name").value)
            if self.peek().kind == ",":
                self.next()
            else:
                self.expect(")")
                return names

    def arguments(self, close):
        values = []
        if self.peek().kind == close:
            self.next()
            return values
        while True:
            values.append(self.expression())
            if self.peek().kind == ",":
                self.next()
            else:
                self.expect(close)
                return values

    def function_body(self):
        self.depth += 1
        body = self.block()
        self.depth -= 1
        return body

    def primary(self):
        token = self.next()
        kind = token.kind
        if kind in ("int", "string"):
            return self.at(token, node="literal", value=token.value)
        if kind in ("true", "false", "nil"):
            return self.at(token, node="literal", value={"true": True, "false": False, "nil": None}[kind])
        if kind == "name":
            return self.at(token, node="name", name=token.value)
        if kind == "(":
            inner = self.expression()
            self.expect(")")
            return inner
        if kind == "[":
            return self.at(token, node="list", items=self.arguments("]"))
        if kind == "fn":
            params = self.params()
            return self.at(token, node="function", name=None, params=params, body=self.function_body())
        self.pos -= 1
        self.unexpected()

    def postfix(self):
        value = self.primary()
        while True:
            token = self.peek()
            if token.kind == "(":
                self.next()
                value = self.at(token, node="call", callee=value, args=self.arguments(")"))
            elif token.kind == "[":
                self.next()
                index = self.expression()
                self.expect("]")
                value = self.at(token, node="index", target=value, index=index)
            else:
                return value

    def unary(self):
        token = self.peek()
        if token.kind in ("-", "not"):
            self.next()
            return self.at(token, node="unary", op=token.kind, operand=self.unary())
        return self.postfix()

    # Each level is a left-grouping loop over its operators.
    def binary(self, level):
        operand = (lambda: self.binary(level + 1)) if level + 1 < len(self.LEVELS) else self.unary
        left = operand()
        while self.peek().kind in self.LEVELS[level]:
            token = self.next()
            left = self.at(token, node="binary", op=token.kind, left=left, right=operand())
        return left

    def expression(self):
        return self.binary(0)

    def block(self):
        opening = self.expect("{")
        body = []
        while True:
            if self.peek().kind == "}":
                self.next()
                return self.at(opening, node="block", body=body)
            if self.peek().kind == "end":
                self.unexpected()
            body.append(self.statement())

    def if_statement(self, token):
        self.expect("(")
        condition = self.expression()
        self.expect(")")
        then = self.block()
        otherwise = None
        if self.peek().kind == "else":
            self.next()
            otherwise = self.if_statement(self.next()) if self.peek().kind == "if" else self.block()
        return self.at(token, node="if", condition=condition, then=then, otherwise=otherwise)

    def statement(self):
        token = self.peek()
        kind = token.kind
        if kind == "let":
            self.next()
            name = self.expect("name").value
            self.expect("=")
            value = self.expression()
            self.expect(";")
            return self.at(token, node="let", name=name, value=value)
        if kind == "print":
            self.next()
            value = self.expression()
            self.expect(";")
            return self.at(token, node="print", value=value)
        if kind == "if":
            return self.if_statement(self.next())
        if kind == "while":
            self.next()
            self.expect("(")
            condition = self.expression()
            self.expect(")")
            return self.at(token, node="while", condition=condition, body=self.block())
        if kind == "return":
            if self.depth == 0:
                raise TinyError("syntax", token.line, token.column)
            self.next()
            if self.peek().kind == ";":
                self.next()
                return self.at(token, node="return", value=None)
            value = self.expression()
            self.expect(";")
            return self.at(token, node="return", value=value)
        if kind == "{":
            return self.block()
        if kind == "fn" and self.ahead().kind == "name":
            self.next()
            name = self.next().value
            params = self.params()
            return self.at(token, node="declare", name=name, params=params, body=self.function_body())
        if kind == "name" and self.ahead().kind == "=":
            self.next()
            self.next()
            value = self.expression()
            self.expect(";")
            return self.at(token, node="assign", name=token.value, value=value)
        value = self.expression()
        self.expect(";")
        return self.at(token, node="expression", value=value)

    def program(self):
        statements = []
        while self.peek().kind != "end":
            statements.append(self.statement())
        return statements


class Scope:
    def __init__(self, parent):
        self.parent = parent
        self.names = {}

    def owner(self, name):
        scope = self
        while scope is not None and name not in scope.names:
            scope = scope.parent
        return scope


def type_of(v):
    # `bool` is a subclass of `int`, so it is tested by exact type.
    if type(v) is bool:
        return "bool"
    if type(v) is int:
        return "int"
    if type(v) is str:
        return "string"
    if type(v) is list:
        return "list"
    if v is None:
        return "nil"
    return "function"


def equal(a, b):
    kind = type_of(a)
    if kind != type_of(b):
        return False
    if kind == "list":
        return len(a) == len(b) and all(equal(x, y) for x, y in zip(a, b))
    if kind == "function":
        return a is b
    return a == b


def show(v, nested):
    kind = type_of(v)
    if kind == "int":
        return str(v)
    if kind == "string":
        return '"' + v + '"' if nested else v
    if kind == "bool":
        return "true" if v else "false"
    if kind == "nil":
        return "nil"
    if kind == "list":
        return "[" + ", ".join(show(item, True) for item in v) + "]"
    return "<fn>" if v.name is None else f"<fn {v.name}>"


class Interpreter:
    def __init__(self):
        self.output = []
        self.globals = Scope(None)
        self.globals.names["len"] = Builtin("len")
        self.globals.names["str"] = Builtin("str")

    @staticmethod
    def fail(kind, node):
        raise TinyError(kind, node["line"], node["column"])

    def boolean(self, node, v):
        if type_of(v) != "bool":
            self.fail("type", node)
        return v

    def call(self, node, callee, args):
        if type_of(callee) != "function":
            self.fail("type", node)
        if isinstance(callee, Builtin):
            if len(args) != 1:
                self.fail("arity", node)
            x = args[0]
            if callee.name == "len":
                if type_of(x) not in ("string", "list"):
                    self.fail("type", node)
                return len(x)
            return show(x, False)
        if len(args) != len(callee.params):
            self.fail("arity", node)
        scope = Scope(callee.scope)
        for name, value in zip(callee.params, args):
            scope.names[name] = value
        signal = self.execute(callee.body["body"], scope)
        return None if signal is None else signal[0]

    def arithmetic(self, node, a, b):
        op = node["op"]
        if op == "+" and type_of(a) == type_of(b) and type_of(a) in ("string", "list"):
            return a + b
        if type_of(a) != "int" or type_of(b) != "int":
            self.fail("type", node)
        if op == "+":
            return a + b
        if op == "-":
            return a - b
        if op == "*":
            return a * b
        if b == 0:
            self.fail("division", node)
        # Truncates toward zero, and the remainder takes the dividend's sign.
        quotient = abs(a) // abs(b)
        if (a < 0) != (b < 0):
            quotient = -quotient
        return quotient if op == "/" else a - b * quotient

    def compare(self, node, a, b):
        kind = type_of(a)
        if kind != type_of(b) or kind not in ("int", "string"):
            self.fail("type", node)
        op = node["op"]
        if op == "<":
            return a < b
        if op == "<=":
            return a <= b
        if op == ">":
            return a > b
        return a >= b

    def evaluate(self, node, scope):
        kind = node["node"]
        if kind == "literal":
            return node["value"]
        if kind == "name":
            owner = scope.owner(node["name"])
            if owner is None:
                self.fail("name", node)
            return owner.names[node["name"]]
        if kind == "list":
            return [self.evaluate(item, scope) for item in node["items"]]
        if kind == "function":
            return Function(None, node["params"], node["body"], scope)
        if kind == "unary":
            v = self.evaluate(node["operand"], scope)
            if node["op"] == "-":
                if type_of(v) != "int":
                    self.fail("type", node)
                return -v
            return not self.boolean(node, v)
        if kind == "binary":
            op = node["op"]
            left = self.evaluate(node["left"], scope)
            if op == "and":
                return self.boolean(node, self.evaluate(node["right"], scope)) if self.boolean(node, left) else False
            if op == "or":
                return True if self.boolean(node, left) else self.boolean(node, self.evaluate(node["right"], scope))
            right = self.evaluate(node["right"], scope)
            if op == "==":
                return equal(left, right)
            if op == "!=":
                return not equal(left, right)
            if op in ("<", "<=", ">", ">="):
                return self.compare(node, left, right)
            return self.arithmetic(node, left, right)
        if kind == "call":
            callee = self.evaluate(node["callee"], scope)
            args = [self.evaluate(arg, scope) for arg in node["args"]]
            return self.call(node, callee, args)
        target = self.evaluate(node["target"], scope)
        index = self.evaluate(node["index"], scope)
        if type_of(target) not in ("list", "string") or type_of(index) != "int":
            self.fail("type", node)
        if index < 0 or index >= len(target):
            self.fail("index", node)
        return target[index]

    # Runs statements in `scope`; returns None, or `(value,)` when a `return`
    # ran.
    def execute(self, statements, scope):
        for statement in statements:
            signal = self.step(statement, scope)
            if signal is not None:
                return signal
        return None

    def step(self, node, scope):
        kind = node["node"]
        if kind == "let":
            scope.names[node["name"]] = self.evaluate(node["value"], scope)
        elif kind == "assign":
            value = self.evaluate(node["value"], scope)
            owner = scope.owner(node["name"])
            if owner is None:
                self.fail("name", node)
            owner.names[node["name"]] = value
        elif kind == "print":
            self.output.append(show(self.evaluate(node["value"], scope), False))
        elif kind == "expression":
            self.evaluate(node["value"], scope)
        elif kind == "block":
            return self.execute(node["body"], Scope(scope))
        elif kind == "declare":
            scope.names[node["name"]] = Function(node["name"], node["params"], node["body"], scope)
        elif kind == "return":
            return (None if node["value"] is None else self.evaluate(node["value"], scope),)
        elif kind == "if":
            condition = self.evaluate(node["condition"], scope)
            if type_of(condition) != "bool":
                self.fail("type", node)
            if condition:
                return self.step(node["then"], scope)
            if node["otherwise"] is not None:
                return self.step(node["otherwise"], scope)
        elif kind == "while":
            while True:
                condition = self.evaluate(node["condition"], scope)
                if type_of(condition) != "bool":
                    self.fail("type", node)
                if not condition:
                    break
                signal = self.step(node["body"], scope)
                if signal is not None:
                    return signal
        return None


def interpret(source):
    program = Parser(lex(source)).program()
    interpreter = Interpreter()
    interpreter.execute(program, interpreter.globals)
    return interpreter.output


def run(source):
    # Tiny recursion nests several Python frames per call, so the program runs
    # on a thread with room for them.
    result = {}

    def target():
        try:
            result["value"] = interpret(source)
        except BaseException as error:  # re-raised on the calling thread
            result["error"] = error

    limit = sys.getrecursionlimit()
    size = threading.stack_size()
    sys.setrecursionlimit(200000)
    threading.stack_size(512 * 1024 * 1024)
    try:
        thread = threading.Thread(target=target)
        thread.start()
        thread.join()
    finally:
        threading.stack_size(size)
        sys.setrecursionlimit(limit)
    if "error" in result:
        raise result["error"]
    return result["value"]
