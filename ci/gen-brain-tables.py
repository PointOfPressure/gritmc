#!/usr/bin/env python3
"""gen-brain-tables.py — every brain provider and activity package in the
26.2 jar's entity package, as data.

Reads the jar with `javap -p -c -constants` (JDK 25) and writes
crates/rustmc-entity/golden/mob_brain_tables.json:

* `providers`: for each class whose static initialiser calls
  `Brain.provider(...)`, the memory-type list, the sensor-type list (each a
  `List.of`/`ImmutableList.of` of `getstatic` tokens, in push order) and the
  activity-supplier token (a lambda, or `null` for the one-argument form);
* `packages`: for each class named `*Ai` or `VillagerGoalPackages`, and for
  each provider class, every method whose body references
  `ai/behavior/`, `ai/ActivityData` or `Brain.addActivity`, folded into an
  expression tree by a linear stack machine — `{"new": C, "ctor": desc,
  "args": [...]}` for constructions, `{"call": "Owner.name:desc",
  "args": [...]}` for calls, `{"lambda": "#n:name:desc", "args": [captures]}`,
  `{"array": [...]}` for varargs, and verbatim bytecode tokens (`0.5f`,
  `getstatic Foo.BAR`, `aload 1`, `iconst 3` …) for everything else. A
  method whose body branches (`if*`, `goto`, `*switch`) cannot be folded by
  a linear walk and is emitted as `{"branches": true, "tokens": [...]}` so
  nothing is invented for it.

No interpretation happens here; the Rust side reads the descriptors.

Usage: gen-brain-tables.py <jar> <out.json>
The regenerate-and-diff wrapper is ci/brain-tables.sh.
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import zipfile

PKG = 'net/minecraft/world/entity/'
BRANCH_RE = re.compile(r'^\s*\d+: (if[a-z_]*|goto|goto_w|tableswitch|lookupswitch|jsr)\b')
INTEREST_RE = re.compile(r'ai/behavior/|ai/ActivityData|ai/Brain\.addActivity|ai/Brain\.provider')


def javap(cls_dir, cls):
    return subprocess.run(['javap', '-p', '-c', '-constants', cls + '.class'], cwd=cls_dir,
                          capture_output=True, text=True, check=True).stdout


def methods(text):
    """Split javap output into [(signature, [instruction lines])] in order."""
    res = []
    cur = None
    for line in text.split('\n'):
        if re.match(r'^  [a-z].*\(.*\);$', line) or re.match(r'^  static \{\};$', line):
            cur = [line.strip(), []]
            res.append(cur)
        elif cur is not None and re.match(r'^\s+\d+: ', line):
            cur[1].append(line.strip())
    return res


def descriptor_params(desc):
    """Parameter count of a JVM method descriptor `(...)R`."""
    inner = desc[1:desc.index(')')]
    i = 0
    n = 0
    while i < len(inner):
        c = inner[i]
        if c == '[':
            i += 1
            continue
        if c == 'L':
            i = inner.index(';', i) + 1
        else:
            i += 1
        n += 1
    return n


def returns_void(desc):
    return desc.endswith(')V')


def returns_wide(desc):
    return desc.endswith(')J') or desc.endswith(')D')


def split_ins(ins):
    body = ins.split(None, 1)[1]
    parts = body.split()
    return parts[0], body, parts


BOXERS = {
    'java/lang/Integer.valueOf:(I)Ljava/lang/Integer;',
    'java/lang/Float.valueOf:(F)Ljava/lang/Float;',
    'java/lang/Double.valueOf:(D)Ljava/lang/Double;',
    'java/lang/Long.valueOf:(J)Ljava/lang/Long;',
    'java/lang/Boolean.valueOf:(Z)Ljava/lang/Boolean;',
}


class Fold:
    """A linear stack machine over one method body."""

    def __init__(self, lines, cls, is_static=False):
        self.lines = lines
        self.cls = cls
        self.is_static = is_static
        self.stack = []
        self.locals = {}
        self.statements = []
        self.result = None

    def push(self, v):
        self.stack.append(v)

    def pop(self):
        if not self.stack:
            raise SystemExit('stack underflow in %s' % self.cls)
        return self.stack.pop()

    def popn(self, n):
        if n == 0:
            return []
        if len(self.stack) < n:
            raise SystemExit('stack underflow (%d needed) in %s: %r' % (n, self.cls, self.stack))
        out = self.stack[-n:]
        del self.stack[-n:]
        return out

    def run(self):
        for ins in self.lines:
            self.step(ins)
        return {'statements': self.statements, 'result': self.result}

    def step(self, ins):
        op, body, parts = split_ins(ins)
        if op == 'aload_0' and not self.is_static:
            self.push('this')
        elif op in ('checkcast', 'nop'):
            pass
        elif op == 'dup':
            self.push(self.stack[-1])
        elif op == 'pop':
            self.pop()
        elif op == 'pop2':
            self.pop()
        elif op == 'swap':
            a = self.pop()
            b = self.pop()
            self.push(a)
            self.push(b)
        elif op.startswith('iconst_'):
            v = op[len('iconst_'):]
            self.push('-1' if v == 'm1' else v)
        elif op in ('bipush', 'sipush'):
            self.push(parts[1])
        elif op in ('dconst_0', 'dconst_1'):
            self.push('double ' + op[-1] + '.0d')
        elif op in ('fconst_0', 'fconst_1', 'fconst_2'):
            self.push('float ' + op[-1] + '.0f')
        elif op in ('lconst_0', 'lconst_1'):
            self.push('long ' + op[-1] + 'l')
        elif op == 'aconst_null':
            self.push('null')
        elif op in ('ldc', 'ldc_w', 'ldc2_w'):
            m = re.search(r'// (.*)$', body)
            self.push(m.group(1).strip())
        elif op == 'getstatic':
            m = re.search(r'// Field (.*):', body)
            self.push('getstatic ' + m.group(1))
        elif op == 'putstatic':
            m = re.search(r'// Field (.*):', body)
            self.statements.append({'putstatic': m.group(1), 'value': self.pop()})
        elif op == 'getfield':
            m = re.search(r'// Field (.*):', body)
            recv = self.pop()
            self.push({'getfield': m.group(1), 'of': recv})
        elif op == 'putfield':
            m = re.search(r'// Field (.*):', body)
            v = self.pop()
            recv = self.pop()
            self.statements.append({'putfield': m.group(1), 'of': recv, 'value': v})
        elif op.startswith('aload') or op.startswith('iload') or op.startswith('fload') or op.startswith('dload') or op.startswith('lload'):
            idx = op.split('_')[1] if '_' in op else parts[1]
            self.push(self.locals.get(idx, op[0] + 'load ' + idx))
        elif op.startswith('astore') or op.startswith('istore') or op.startswith('fstore') or op.startswith('dstore') or op.startswith('lstore'):
            idx = op.split('_')[1] if '_' in op else parts[1]
            self.locals[idx] = self.pop()
        elif op == 'new':
            m = re.search(r'// class (.*)$', body)
            self.push({'new': m.group(1)})
        elif op == 'anewarray':
            self.pop()
            self.push({'array': []})
        elif op == 'newarray':
            self.pop()
            self.push({'array': []})
        elif op in ('aastore', 'iastore', 'fastore', 'dastore', 'lastore', 'bastore'):
            v = self.pop()
            self.pop()  # index
            arr = self.pop()  # the dup'd array reference
            if not (isinstance(arr, dict) and 'array' in arr):
                raise SystemExit('%s without an array under it in %s: %r' % (op, self.cls, self.stack))
            arr['array'].append(v)
        elif op == 'arraylength':
            self.pop()
            self.push('arraylength')
        elif op == 'invokedynamic':
            m = re.search(r'InvokeDynamic #(\d+):(\w+):(\S+)', body)
            desc = m.group(3)
            args = self.popn(descriptor_params(desc))
            self.push({'lambda': '#%s:%s:%s' % (m.group(1), m.group(2), desc), 'args': args})
        elif op in ('invokestatic', 'invokevirtual', 'invokeinterface', 'invokespecial'):
            m = re.search(r'// (?:Interface)?Method (.*)$', body)
            full = m.group(1)
            owner_name, desc = full.split(':', 1)
            n = descriptor_params(desc)
            if op == 'invokespecial' and '."<init>"' in owner_name:
                args = self.popn(n)
                marker = self.pop()
                owner = owner_name[:owner_name.index('."<init>"')]
                if isinstance(marker, dict) and marker.get('new') == owner:
                    # `new C; dup; ...; invokespecial <init>` leaves the dup'd
                    # reference as the constructed value
                    if self.stack and self.stack[-1] is marker:
                        self.pop()
                    self.push({'new': owner, 'ctor': desc, 'args': args})
                else:
                    # super(...) / this(...) inside a constructor
                    self.statements.append({'call': full, 'args': args, 'on': marker})
                return
            args = self.popn(n)
            recv = None if op == 'invokestatic' else self.pop()
            if full in BOXERS:
                self.push(args[0])
                return
            node = {'call': full, 'args': args}
            if recv is not None:
                node['on'] = recv
            if returns_void(desc):
                self.statements.append(node)
            else:
                self.push(node)
        elif op in ('areturn', 'ireturn', 'freturn', 'dreturn', 'lreturn'):
            self.result = self.pop()
        elif op == 'return':
            pass
        elif op in ('i2f', 'i2d', 'i2l', 'f2d', 'd2f', 'l2i', 'f2i', 'd2i', 'i2b', 'i2c', 'i2s', 'l2d', 'd2l', 'f2l', 'l2f'):
            v = self.pop()
            self.push({'conv': op, 'of': v})
        elif op in ('iadd', 'isub', 'imul', 'idiv', 'fadd', 'fsub', 'fmul', 'fdiv', 'dadd', 'dsub', 'dmul', 'ddiv', 'ladd', 'lsub', 'lmul', 'ldiv', 'irem', 'iand', 'ior', 'ixor', 'ishl', 'ishr', 'iushr', 'land', 'lor'):
            b = self.pop()
            a = self.pop()
            self.push({'op': op, 'args': [a, b]})
        elif op in ('ineg', 'fneg', 'dneg', 'lneg'):
            a = self.pop()
            self.push({'op': op, 'args': [a]})
        elif op == 'instanceof':
            m = re.search(r'// class (.*)$', body)
            a = self.pop()
            self.push({'instanceof': m.group(1), 'of': a})
        elif op == 'athrow':
            self.pop()
        elif op == 'iinc':
            pass
        elif op == 'monitorenter' or op == 'monitorexit':
            self.pop()
        else:
            raise SystemExit('unhandled opcode %s in %s: %s' % (op, self.cls, ins))


def raw_tokens(lines):
    out = []
    for ins in lines:
        op, body, parts = split_ins(ins)
        m = re.search(r'// (?:Interface)?Method (.*)$', body) or re.search(r'// Field (.*):', body) or re.search(r'// (.*)$', body)
        out.append(op + (' ' + m.group(1).strip() if m else (' ' + ' '.join(parts[1:]) if len(parts) > 1 and not parts[1].startswith('#') else '')))
    return out


def fold_method(sig, lines, cls):
    if any(BRANCH_RE.match(l) for l in lines):
        return {'branches': True, 'tokens': raw_tokens(lines)}
    is_static = ' static ' in sig or sig == 'static {};'
    return Fold(lines, cls, is_static).run()


def find_provider(folded):
    """The Brain.provider(...) call in a static initialiser's statements."""
    def walk(node):
        if isinstance(node, dict):
            if 'call' in node and node['call'].startswith('net/minecraft/world/entity/ai/Brain.provider:'):
                yield node
            for k in ('args', 'value', 'of', 'on'):
                if k in node:
                    v = node[k]
                    if isinstance(v, list):
                        for x in v:
                            yield from walk(x)
                    else:
                        yield from walk(v)
            if 'array' in node:
                for x in node['array']:
                    yield from walk(x)
        elif isinstance(node, list):
            for x in node:
                yield from walk(x)
    for st in folded.get('statements', []):
        for hit in walk(st):
            return hit
    return None


def list_tokens(node):
    """A `List.of(...)` / `ImmutableList.of(...)` / `ImmutableSet.of(...)` of getstatics, flattened."""
    if isinstance(node, dict) and 'call' in node and re.search(r'(List|Set|ImmutableList|ImmutableSet)\.of:', node['call']):
        out = []
        for a in node['args']:
            if isinstance(a, dict) and 'array' in a:
                out.extend(a['array'])
            else:
                out.append(a)
        return out
    return node


def main():
    jar, out = sys.argv[1], sys.argv[2]
    work = tempfile.mkdtemp()
    with zipfile.ZipFile(jar) as z:
        names = [n for n in z.namelist() if n.startswith(PKG) and n.endswith('.class')]
        z.extractall(work, names)
    providers = {}
    packages = {}
    for n in sorted(names):
        cls = n[:-len('.class')]
        short = cls[len(PKG):]
        with open(os.path.join(work, n), 'rb') as f:
            raw = f.read()
        is_ai = re.search(r'(Ai|GoalPackages)(\$[A-Za-z0-9]+)?$', short) is not None
        has_provider = b'Brain$Provider' in raw and not short.startswith('ai/')
        if not (is_ai or has_provider):
            continue
        text = javap(work, cls)
        ms = methods(text)
        for sig, lines in ms:
            joined = '\n'.join(lines)
            if sig == 'static {};' and 'ai/Brain.provider' in joined:
                folded = fold_method(sig, lines, cls)
                if folded.get('branches'):
                    providers[short] = {'branches': True, 'tokens': folded['tokens']}
                    continue
                call = find_provider(folded)
                if call is None:
                    continue
                args = call['args']
                if len(args) == 1:
                    providers[short] = {'memories': [], 'sensors': list_tokens(args[0]), 'supplier': None}
                elif len(args) == 2:
                    providers[short] = {'memories': [], 'sensors': list_tokens(args[0]), 'supplier': args[1]}
                else:
                    providers[short] = {'memories': list_tokens(args[0]), 'sensors': list_tokens(args[1]), 'supplier': args[2]}
                continue
            if INTEREST_RE.search(joined):
                packages.setdefault(short, {})[sig] = fold_method(sig, lines, cls)
    doc = {
        'source': os.path.basename(jar),
        'note': 'generated by ci/gen-brain-tables.py; tokens are verbatim bytecode operands, trees are the linear fold of each method body',
        'providers': providers,
        'packages': packages,
    }
    with open(out, 'w') as f:
        json.dump(doc, f, indent=1, sort_keys=True)
        f.write('\n')
    print('%d providers, %d classes with %d activity/behaviour methods -> %s' % (
        len(providers), len(packages), sum(len(v) for v in packages.values()), out))


if __name__ == '__main__':
    main()
