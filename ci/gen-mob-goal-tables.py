#!/usr/bin/env python3
"""gen-mob-goal-tables.py — every `registerGoals()` and attribute builder in the
26.2 jar's entity package, as data.

Reads the jar with `javap -p -c -constants` (JDK 25) and writes
crates/rustmc-entity/golden/mob_goal_tables.json: for each class that
declares `registerGoals()`, the ordered `addGoal` calls with their selector
(`goalSelector` / `targetSelector`), priority, goal class, constructor
descriptor and the operand tokens pushed for it, in bytecode order; plus,
for each class that declares a static `create*Attributes()` builder, the
base builder call and the `(attribute, value)` pairs added.

Everything is a verbatim bytecode token: `1.5d`, `8.0f`, `iconst_1`,
`class net/minecraft/world/entity/player/Player`, `getstatic Foo.BAR`,
`lambda#0`, `this`, `super.registerGoals()`. No interpretation happens here;
the Rust side reads the descriptor to know which token is which.

Usage: gen-mob-goal-tables.py <jar> <out.json>
The regenerate-and-diff wrapper is ci/mob-goal-tables.sh.
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import zipfile

PKG = 'net/minecraft/world/entity/'
GOAL_RE = re.compile(r'(ai/goal/|Goal$|Goal\$)')


def javap(cls_dir, cls):
    out = subprocess.run(['javap', '-p', '-c', '-constants', cls + '.class'], cwd=cls_dir,
                         capture_output=True, text=True, check=True).stdout
    return out


def methods(text):
    """Split javap output into {signature: [instruction lines]}."""
    res = {}
    cur = None
    for line in text.split('\n'):
        if re.match(r'^  [a-z].*\(.*\);$', line) or re.match(r'^  static \{\};$', line):
            cur = line.strip()
            res[cur] = []
        elif cur is not None and re.match(r'^\s+\d+: ', line):
            res[cur].append(line.strip())
    return res


def token(ins):
    """One operand-pushing instruction -> a token, or None for stack plumbing."""
    op = ins.split()[1] if ins.split()[0].endswith(':') else ins.split()[0]
    body = ins.split(None, 1)[1] if ins.split()[0].endswith(':') else ins
    parts = body.split()
    op = parts[0]
    if op == 'aload_0':
        return 'this'
    if op in ('dup', 'pop', 'checkcast', 'iastore', 'newarray'):
        return None
    if op == 'anewarray':
        return 'anewarray'
    if op == 'aastore':
        return 'aastore'
    if op.startswith('astore'):
        return 'astore ' + (op.split('_')[1] if '_' in op else parts[1])
    if op.startswith('iconst_'):
        v = op[len('iconst_'):]
        return '-1' if v == 'm1' else v
    if op == 'bipush' or op == 'sipush':
        return parts[1]
    if op in ('dconst_0', 'dconst_1'):
        return 'double ' + op[-1] + '.0d'
    if op in ('fconst_0', 'fconst_1', 'fconst_2'):
        return 'float ' + op[-1] + '.0f'
    if op == 'lconst_0' or op == 'lconst_1':
        return 'long ' + op[-1] + 'l'
    if op == 'aconst_null':
        return 'null'
    if op in ('ldc', 'ldc_w', 'ldc2_w'):
        m = re.search(r'// (.*)$', body)
        return m.group(1).strip() if m else body
    if op == 'getstatic':
        m = re.search(r'// Field (.*):', body)
        return 'getstatic ' + m.group(1)
    if op == 'invokedynamic':
        m = re.search(r'InvokeDynamic #(\d+):(\w+):(\S+)', body)
        return 'lambda#%s:%s:%s' % (m.group(1), m.group(2), m.group(3))
    if op in ('invokestatic', 'invokevirtual', 'invokeinterface', 'invokespecial'):
        m = re.search(r'// (?:Interface)?Method (.*)$', body)
        return ('callstatic ' if op == 'invokestatic' else 'call ') + m.group(1)
    if op == 'new':
        m = re.search(r'// class (.*)$', body)
        return 'new ' + m.group(1)
    if op.startswith('aload'):
        return 'aload ' + (op.split('_')[1] if '_' in op else parts[1])
    if op.startswith('iload') or op.startswith('dload') or op.startswith('fload'):
        return op
    if op == 'getfield':
        m = re.search(r'// Field (.*):', body)
        return 'getfield ' + m.group(1)
    if op == 'putfield':
        m = re.search(r'// Field (.*):', body)
        return 'putfield ' + m.group(1)
    return op


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


def parse_register_goals(lines, cls):
    """The addGoal sequence of one registerGoals body."""
    goals = []
    selector = None
    priority = None
    stack = []  # nested `new` constructs: [class, tokens]
    pending = None  # the finished goal construct awaiting addGoal
    fields = {}  # goals stored in a field before being added (`this.temptGoal = new ...`)
    locals_ = {}  # goals stored in a local first (`astore_1` ... `aload_1`)
    for ins in lines:
        t = token(ins)
        if t is None:
            continue
        if t.startswith('getfield ') and (t.endswith('goalSelector') or t.endswith('targetSelector')):
            selector = t.split()[-1].split('.')[-1]
            continue
        if t.startswith('astore ') and pending is not None and not stack:
            locals_[t.split()[1]] = pending
            pending = None
            continue
        if t.startswith('aload ') and not stack and selector is not None and priority is not None and t.split()[1] in locals_:
            pending = dict(locals_[t.split()[1]])
            pending['local'] = t.split()[1]
            continue
        if t.startswith('putfield ') and pending is not None and not stack:
            fields[t.split()[-1].split('.')[-1]] = pending
            pending = None
            continue
        if t.startswith('getfield ') and not stack and selector is not None and priority is not None:
            name = t.split()[-1].split('.')[-1]
            pending = dict(fields.get(name, {'class': 'field:' + name, 'ctor': '?', 'args': []}))
            pending['field'] = name
            continue
        if t == 'call net/minecraft/world/entity/ai/goal/GoalSelector.addGoal:(ILnet/minecraft/world/entity/ai/goal/Goal;)V':
            if pending is None:
                goals.append({'selector': selector, 'priority': priority, 'class': '?', 'ctor': '?', 'args': ['<unparsed>']})
            else:
                pending['selector'] = selector
                pending['priority'] = priority
                goals.append(pending)
            pending = None
            priority = None
            continue
        if t.startswith('call ') and 'registerGoals:()V' in t:
            goals.append({'selector': None, 'priority': None, 'class': 'super', 'ctor': t[5:], 'args': []})
            continue
        if t.startswith('new '):
            stack.append({'class': t[4:], 'args': []})
            continue
        if t.startswith('call ') and '."<init>":' in t:
            m = re.match(r'call (.*)\."<init>":(.*)$', t)
            if stack and stack[-1]['class'] == m.group(1):
                done = stack.pop()
                done['ctor'] = m.group(2)
                if stack:
                    stack[-1]['args'].append({'class': done['class'], 'ctor': done['ctor'], 'args': done['args']})
                else:
                    pending = done
                continue
        if stack:
            args = stack[-1]['args']
            if t == 'anewarray':
                args.pop()  # the array length
                args.append({'array': []})
                continue
            if t == 'aastore':
                # everything after the array marker is `index, value-expression...`
                m = max(i for i, a in enumerate(args) if isinstance(a, dict) and 'array' in a)
                tail = args[m + 1:]
                del args[m + 1:]
                if not tail:
                    raise SystemExit('aastore without an index in %s: %r' % (cls, args))
                value = tail[1:]
                args[m]['array'].append(value[0] if len(value) == 1 else ' '.join(str(v) for v in value))
                continue
            if (t.startswith('call ') or t.startswith('callstatic ') or t.startswith('lambda#')) and '"<init>"' not in t:
                # fold a method-call expression (or a lambda with its captures)
                # and its operands into one token
                n = descriptor_params(t[t.index('(') - 0:] if t.startswith('lambda#') else t.split(':', 1)[1])
                if t.startswith('call '):
                    n += 1
                operands = args[len(args) - n:] if n else []
                del args[len(args) - n:]
                args.append(' '.join(str(a) for a in operands + [t]) if operands else t)
                continue
            args.append(t)
            continue
        if pending is not None:
            pending.setdefault('chain', []).append(t)
            continue
        if selector is not None and priority is None and re.match(r'^-?\d+$', t):
            priority = int(t)
            continue
    return goals


def parse_attributes(lines):
    """A static create*Attributes body: base call and (attribute, value) adds."""
    base = None
    adds = []
    cur_attr = None
    cur_val = None
    for ins in lines:
        t = token(ins)
        if t is None:
            continue
        if (t.startswith('callstatic ') or t.startswith('call ')) and 'Attributes:()' in t and base is None:
            base = t.split(' ', 1)[1]
            continue
        if t.startswith('getstatic net/minecraft/world/entity/ai/attributes/Attributes.'):
            cur_attr = t.split('.')[-1]
            continue
        if t.startswith('call ') and 'AttributeSupplier$Builder.add:' in t:
            adds.append({'attribute': cur_attr, 'value': cur_val})
            cur_attr = None
            cur_val = None
            continue
        if cur_attr is not None and cur_val is None:
            cur_val = t
    return {'base': base, 'adds': adds}


def main():
    jar, out = sys.argv[1], sys.argv[2]
    work = tempfile.mkdtemp()
    with zipfile.ZipFile(jar) as z:
        names = [n for n in z.namelist() if n.startswith(PKG) and n.endswith('.class') and '/ai/' not in n]
        z.extractall(work, names)
    mobs = {}
    attrs = {}
    for n in sorted(names):
        cls = n[:-len('.class')]
        text = javap(work, cls)
        ms = methods(text)
        short = cls[len(PKG):]
        for sig, lines in ms.items():
            if sig == 'protected void registerGoals();' or sig == 'public void registerGoals();':
                mobs[short] = parse_register_goals(lines, cls)
            m = re.match(r'^(?:public |protected |private )?static net\.minecraft\.world\.entity\.ai\.attributes\.AttributeSupplier\$Builder (\w+)\(\);$', sig)
            if m:
                attrs.setdefault(short, {})[m.group(1)] = parse_attributes(lines)
    doc = {
        'source': os.path.basename(jar),
        'note': 'generated by ci/gen-mob-goal-tables.py; tokens are verbatim bytecode operands in push order',
        'register_goals': mobs,
        'attributes': attrs,
    }
    with open(out, 'w') as f:
        json.dump(doc, f, indent=1, sort_keys=True)
        f.write('\n')
    print('%d registerGoals, %d attribute builders -> %s' % (len(mobs), sum(len(v) for v in attrs.values()), out))


if __name__ == '__main__':
    main()
