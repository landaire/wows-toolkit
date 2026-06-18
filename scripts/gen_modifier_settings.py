# -*- coding: utf-8 -*-
"""Generate the Rust modifier value-formatting table from the client's
MODIFIER_SETTINGS dict plus the ModifierValueType (MVT) isPositive registry.

Emits crates/wowsunpack/src/game_params/modifier_settings_data.rs: a generated,
version-gated table mirroring scripts/gen_skill_grid_rs.py. The toolkit compiles
it in so there is no runtime/data dependency.

Sources (deob client build, default 11791718):
  - ModifierSettings.py: AST-parsed for the MODIFIER_SETTINGS dict and the
    Measures class. NEVER exec'd -- it does not import standalone.
  - ModifierValueType: the MVT name -> isPositive registry. The shipped file is
    an obfuscated .pyc that stock marshal/uncompyle6 cannot read, so for a .pyc
    input we deobfuscate it with wowsdeob, disassemble with xdis (pydisasm), and
    parse the create(...) calls. A plain .py input is AST-parsed instead.

Usage:
  python scripts/gen_modifier_settings.py [ModifierSettings.py] [ModifierValueType.pyc] [build]

Defensive by design: a per-entry parse failure is logged to stderr and skipped;
the script never crashes on one bad entry. Re-run per game version.

When a formatting-critical numeric arg (multiplier, baseValue, roundDigits) is a
non-literal expression (e.g. ConstantsShip.BW_TO_BALLISTIC/1000, 1/KM_TO_M) it
cannot be resolved here, so the entry is tagged Transform::LabelOnly (label shown
without a wrong number) and counted as unresolved_value -- it is NOT silently
treated as 1.0/0.0.
"""
import ast
import io
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
RS_OUT = os.path.join(ROOT, "crates", "wowsunpack", "src", "game_params", "modifier_settings_data.rs")

DEFAULT_MS = r"G:/deob/scripts/mbf4783af/ModifierSettings.py"
DEFAULT_MVT = r"G:/deob/scripts/Modifiers/ModifierValueType.pyc"
DEFAULT_BUILD = 11791718

# Tools for decompiling the obfuscated MVT .pyc. Overridable via env vars.
WOWSDEOB = os.environ.get("WOWSDEOB", r"G:/dev/wowsdeob/target/release/wowsdeob.exe")
PYDISASM = os.environ.get(
    "PYDISASM",
    r"C:/Users/lander/AppData/Local/Programs/Python/Python38-32/Scripts/pydisasm.exe",
)

# Measures.<NAME> string-constant -> Rust Measure::<Variant>. The Rust enum only
# emits variants actually referenced by the table, plus None.
MEASURE_VARIANTS = {
    "CANTIMETER": "Cantimeter",
    "MILLIMETER": "Millimeter",
    "METER": "Meter",
    "TONN": "Tonn",
    "SECOND": "Second",
    "SECOND_SPACE": "SecondSpace",
    "MINUTE": "Minute",
    "KILOMETER": "Kilometer",
    "KILOMETER_SPACE": "KilometerSpace",
    "KILOMETER_HOUR": "KilometerHour",
    "HORSE_POWER": "HorsePower",
    "KNOTS_SPACE": "KnotsSpace",
    "KNOTS_SHORT": "KnotsShort",
    "SQUADRONS": "Squadrons",
    "AIRPLANES": "Airplanes",
    "AIR_FLIGHT": "AirFlight",
    "PERCENT": "Percent",
    "DEGREE_SECOND": "DegreeSecond",
    "SHOT_MINUTE": "ShotMinute",
    "UNITS": "Units",
    "HP_SECOND": "HpSecond",
    "METER_SECOND": "MeterSecond",
    "TORPEDO_BOMBER": "TorpedoBomber",
    "BOMBER": "Bomber",
    "FIGHTER": "Fighter",
    "SMOKE_CHARGE": "SmokeCharge",
    "CHARGE": "Charge",
    "PERCENT_SECOND": "PercentSecond",
    "UNITS_SECOND": "UnitsSecond",
    "UNITS_SQUARE": "UnitsSquare",
    "DEGREE": "Degree",
    "NONE": "None",
    "SHIP_ICONS": "ShipIcons",
}

# Measure variant -> (IDS key or None, space_before). space_before is false only
# for Percent. Variants whose Measures value is '' or a format string have no key.
MEASURE_IDS = {
    "Cantimeter": "IDS_CANTIMETER",
    "Millimeter": "IDS_MILLIMETER",
    "Meter": "IDS_METER",
    "Tonn": "IDS_TONN",
    "Second": "IDS_SECOND",
    "SecondSpace": "IDS_SECOND_SPACE",
    "Minute": "IDS_MINUTE",
    "Kilometer": "IDS_KILOMETER",
    "KilometerSpace": "IDS_KILOMETER_SPACE",
    "KilometerHour": "IDS_KILOMETER_HOUR",
    "HorsePower": "IDS_HORSE_POWER",
    "KnotsSpace": "IDS_KNOT_SPACE",
    "KnotsShort": "IDS_KNOT",
    "Squadrons": "IDS_PL_SQUADRONS",
    "Airplanes": "IDS_PL_AIRPLANES",
    "AirFlight": "IDS_PL_AIR_FLIGHT",
    "Percent": "IDS_PERCENT",
    "DegreeSecond": "IDS_DEGREE_SECOND",
    "ShotMinute": "IDS_SHOT_MINUTE",
    "Units": "IDS_UNITS",
    "HpSecond": "IDS_HP_SECOND",
    "MeterSecond": "IDS_METER_SECOND",
    "TorpedoBomber": "IDS_CREW_SKILL_MEASURE_TORPEDO_BOMBER",
    "Bomber": "IDS_CREW_SKILL_MEASURE_BOMBER",
    "Fighter": "IDS_CREW_SKILL_MEASURE_FIGHTER",
    "SmokeCharge": "IDS_CREW_SKILL_MEASURE_SMOKE_CHARGE",
    "Charge": "IDS_CREW_SKILL_MEASURE_CHARGE",
    "PercentSecond": "IDS_PERCENT_SECOND",
    "UnitsSecond": "IDS_UNITS_SECOND",
    "UnitsSquare": "IDS_UNITS_SQUARE",
    "Degree": "IDS_DEGREE",
    "None": None,
    "ShipIcons": None,
}


def warn(msg):
    sys.stderr.write("warn: " + msg + "\n")


# Deob getter/valueConverter function name -> ported Rust Transform variant. Only
# pure value math with no runtime dependency is listed; the rest stay LabelOnly.
# __valueConverterNewWaterline is `return abs(value)`.
PORTABLE_TRANSFORMS = {
    "__valueConverterNewWaterline": "Abs",
    "_ModifierSettings__valueConverterNewWaterline": "Abs",
}


def transform_fn_name(node):
    """The bare function name from a getter/valueConverter arg node.

    The deob stores these as references to class-private methods, parsed as a
    Name (`__valueConverterX`) or an Attribute (`self.__valueConverterX`)."""
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return node.attr
    return None


def unquote_disasm_arg(s):
    """Strip a single layer of matching quotes from a disasm opcode arg.

    Different xdis/pydisasm versions render a string operand as either
    'isPositive' or "isPositive"; comparisons must be quote-style agnostic."""
    s = s.strip()
    if len(s) >= 2 and s[0] in "'\"" and s[-1] == s[0]:
        return s[1:-1]
    return s


def parse_mvt_from_disasm(text):
    """Parse xdis disassembly of the deobfuscated ModifierValueType module.

    Each value type is registered as
        ModifierValueType.create(<name>, _ValueGroup.X, isPositive=BOOL, ...)
        ModifierValueType.<name> = <result>     # STORE_ATTR <name>
    isPositive defaults to True when the keyword is absent.
    """
    ins = []
    for ln in text.splitlines():
        m = re.match(r"\s*(?:>>)?\s*(\d+)\s+([A-Z_]+)\s*(?:\((.*)\))?\s*$", ln)
        if m:
            ins.append((m.group(2), (m.group(3) or "").strip()))
    res = {}
    n = len(ins)
    i = 0
    while i < n:
        op, arg = ins[i]
        if op == "LOAD_ATTR" and unquote_disasm_arg(arg) == "create":
            is_pos = True
            pending = None
            name = None
            j = i + 1
            while j < n:
                o2, a2 = ins[j]
                if o2 == "CALL_FUNCTION":
                    k = j + 1
                    while k < n and ins[k][0] not in ("STORE_ATTR", "LOAD_ATTR"):
                        k += 1
                    if k < n and ins[k][0] == "STORE_ATTR":
                        name = unquote_disasm_arg(ins[k][1])
                    break
                if o2 == "LOAD_CONST" and unquote_disasm_arg(a2) == "isPositive":
                    pending = "isPositive"
                elif pending == "isPositive" and o2 in ("LOAD_NAME", "LOAD_GLOBAL", "LOAD_CONST"):
                    is_pos = unquote_disasm_arg(a2) in ("True", "1")
                    pending = None
                j += 1
            if name:
                res[name] = is_pos
            i = j + 1
            continue
        i += 1
    return res


def mvt_from_pyc(pyc_path):
    work = tempfile.mkdtemp(prefix="mvt_deob_")
    try:
        subprocess.run([WOWSDEOB, pyc_path, work], check=True, capture_output=True)
    except Exception as e:
        warn("wowsdeob failed on %s: %s" % (pyc_path, e))
        return {}
    deob = None
    for fn in os.listdir(work):
        if fn.endswith("_stage2_deob.pyc"):
            deob = os.path.join(work, fn)
            break
    if not deob:
        warn("wowsdeob produced no stage2_deob pyc for %s" % pyc_path)
        return {}
    try:
        out = subprocess.run([PYDISASM, deob], check=True, capture_output=True)
    except Exception as e:
        warn("pydisasm failed: %s" % e)
        return {}
    text = out.stdout.decode("utf-8", "replace")
    return parse_mvt_from_disasm(text)


def mvt_from_py(py_path):
    """AST-parse a plain .py MVT module for name -> isPositive (best effort)."""
    src = io.open(py_path, encoding="utf-8", errors="replace").read()
    tree = ast.parse(src)
    res = {}
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        fn = node.func
        if not (isinstance(fn, ast.Attribute) and fn.attr == "create"):
            continue
        if not node.args:
            continue
        name = literal_str(node.args[0])
        if name is None:
            continue
        is_pos = True
        for kw in node.keywords:
            if kw.arg == "isPositive":
                v = literal_bool(kw.value)
                if v is not None:
                    is_pos = v
        res[name] = is_pos
    return res


def build_mvt_map(path):
    if path.endswith(".pyc"):
        return mvt_from_pyc(path)
    res = mvt_from_py(path)
    if not res:
        warn("no MVT entries parsed from %s; isPositive defaults to True" % path)
    return res


def literal_str(node):
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    return None


def literal_bool(node):
    if isinstance(node, ast.Constant) and isinstance(node.value, bool):
        return node.value
    return None


def literal_num(node):
    """A numeric literal, allowing a leading unary minus."""
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
        v = literal_num(node.operand)
        return None if v is None else -v
    if isinstance(node, ast.Constant) and isinstance(node.value, (int, float)) and not isinstance(node.value, bool):
        return float(node.value)
    return None


def parse_translations(node):
    """A `translations` arg (ast.Dict of float-literal keys -> string-literal
    values, or None/absent) -> list of (float, str) pairs. Empty when absent or
    unparseable. Negative keys (USub over a constant) are handled by literal_num.
    """
    if node is None:
        return []
    if isinstance(node, ast.Constant) and node.value is None:
        return []
    if not isinstance(node, ast.Dict):
        warn("translations arg is not a dict literal; ignoring")
        return []
    pairs = []
    for k, v in zip(node.keys, node.values):
        kv = literal_num(k)
        sv = literal_str(v)
        if kv is None or sv is None:
            warn("translations entry not (float -> str) literal; skipping")
            continue
        pairs.append((kv, sv))
    return pairs


def key_name(node):
    """Modifier name from a dict key node.

    'literal'           -> the literal
    MVT.<x>.name        -> <x>
    <x>.name            -> <x>
    """
    s = literal_str(node)
    if s is not None:
        return s
    # X.name  (Attribute attr == 'name')
    if isinstance(node, ast.Attribute) and node.attr == "name":
        inner = node.value
        # MVT.<x>.name : inner is Attribute MVT.<x>
        if isinstance(inner, ast.Attribute):
            return inner.attr
        if isinstance(inner, ast.Name):
            return inner.id
    return None


def measure_variant(node):
    """Measures.<NAME> -> Rust variant; default Meter on failure (logged)."""
    if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name) and node.value.id == "Measures":
        v = MEASURE_VARIANTS.get(node.attr)
        if v is None:
            warn("unknown Measures.%s; using None" % node.attr)
            return "None"
        return v
    warn("measure arg is not Measures.<X>; using None")
    return "None"


def value_type_positive(node, mvt_map, stats):
    """Resolve `positive` from a valueType arg.

    ValueTypeDummy(<bool>?, ...) -> first positional bool (default True)
    MVT.<name> / <name>          -> mvt_map[<name>] (default True, counted)
    """
    if node is None:
        return True
    if isinstance(node, ast.Call):
        fn = node.func
        is_dummy = (isinstance(fn, ast.Name) and fn.id == "ValueTypeDummy") or (
            isinstance(fn, ast.Attribute) and fn.attr == "ValueTypeDummy"
        )
        if is_dummy:
            if node.args:
                b = literal_bool(node.args[0])
                if b is not None:
                    return b
            return True
        # other calls (rare) -- default True
        return True
    # MVT.<name> or bare <name>
    name = None
    if isinstance(node, ast.Attribute):
        name = node.attr
    elif isinstance(node, ast.Name):
        name = node.id
    if name is None:
        return True
    if name in mvt_map:
        return mvt_map[name]
    stats["mvt_unresolved"] += 1
    return True


def parse_ms_call(call, mvt_map, stats):
    """An MS(...) call -> dict of fields, applying constructor defaults. Returns
    None if `call` is not an MS/ModifierSettings call."""
    fn = call.func
    is_ms = (isinstance(fn, ast.Name) and fn.id in ("MS", "ModifierSettings")) or (
        isinstance(fn, ast.Attribute) and fn.attr in ("MS", "ModifierSettings")
    )
    if not is_ms:
        return None

    # ModifierSettings(measure, baseValue, displaySign=True, multiplier=1.0,
    #   getter=None, valueConverter=None, translations=None, hidden=False,
    #   measureValueHidden=False, sortIndex=None, roundDigits=1,
    #   roundPercents=False, valueType=None)
    POSITIONAL = [
        "measure", "baseValue", "displaySign", "multiplier", "getter",
        "valueConverter", "translations", "hidden", "measureValueHidden",
        "sortIndex", "roundDigits", "roundPercents", "valueType",
    ]
    args = {}
    for i, a in enumerate(call.args):
        if i < len(POSITIONAL):
            args[POSITIONAL[i]] = a
    for kw in call.keywords:
        if kw.arg:
            args[kw.arg] = kw.value

    out = {
        "measure": "Meter",
        "base_value": 0.0,
        "display_sign": True,
        "multiplier": 1.0,
        "round_digits": 1,
        "round_percents": False,
        "hidden": False,
        "measure_value_hidden": False,
        "positive": True,
        "transform": "None",
        "translations": [],
    }

    if "measure" in args:
        out["measure"] = measure_variant(args["measure"])
    else:
        warn("MS call missing measure; using Meter")

    # A formatting-critical numeric arg that is present but non-literal cannot be
    # resolved here; mark the entry LabelOnly so we never render a wrong number.
    unresolved_field = None

    bv = literal_num(args.get("baseValue"))
    if bv is not None:
        out["base_value"] = bv
    elif "baseValue" in args:
        unresolved_field = "baseValue"

    ds = literal_bool(args.get("displaySign"))
    if ds is not None:
        out["display_sign"] = ds

    mul = literal_num(args.get("multiplier"))
    if mul is not None:
        out["multiplier"] = mul
    elif "multiplier" in args:
        unresolved_field = "multiplier"

    rd = literal_num(args.get("roundDigits"))
    if rd is not None:
        out["round_digits"] = int(rd)
    elif "roundDigits" in args:
        unresolved_field = "roundDigits"

    rp = literal_bool(args.get("roundPercents"))
    if rp is not None:
        out["round_percents"] = rp

    h = literal_bool(args.get("hidden"))
    if h is not None:
        out["hidden"] = h

    mvh = literal_bool(args.get("measureValueHidden"))
    if mvh is not None:
        out["measure_value_hidden"] = mvh

    out["positive"] = value_type_positive(args.get("valueType"), mvt_map, stats)

    out["translations"] = parse_translations(args.get("translations"))

    # A getter or valueConverter means the displayed value is transformed. Pure
    # value math (no shipParams/GameParams/FieldParams) is ported to a Transform
    # variant; everything else stays LabelOnly (a wrong number is worse than no
    # number). PORTABLE_TRANSFORMS maps the deob function name to the variant.
    if "getter" in args or "valueConverter" in args:
        fn_node = args.get("getter") if "getter" in args else args.get("valueConverter")
        fn_name = transform_fn_name(fn_node)
        ported = PORTABLE_TRANSFORMS.get(fn_name)
        if ported is not None:
            out["transform"] = ported
        else:
            out["transform"] = "LabelOnly"
            stats["label_only"] += 1

    if unresolved_field is not None:
        out["transform"] = "LabelOnly"
        out["unresolved_field"] = unresolved_field
        stats["unresolved_value"] += 1

    return out


def find_modifier_settings_dict(ms_path):
    src = io.open(ms_path, encoding="utf-8", errors="replace").read()
    tree = ast.parse(src)
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign):
            for t in node.targets:
                if isinstance(t, ast.Name) and t.id == "MODIFIER_SETTINGS":
                    if isinstance(node.value, ast.Dict):
                        return node.value
    return None


def extract_entries(ms_path, mvt_map, stats):
    d = find_modifier_settings_dict(ms_path)
    if d is None:
        warn("MODIFIER_SETTINGS dict not found in %s" % ms_path)
        return []
    entries = []
    seen = set()
    for k, v in zip(d.keys, d.values):
        name = key_name(k)
        if name is None:
            warn("skip entry: unparseable key %r" % ast.dump(k))
            continue
        if not isinstance(v, ast.Call):
            warn("skip %s: value is not a call" % name)
            continue
        try:
            fields = parse_ms_call(v, mvt_map, stats)
        except Exception as e:
            warn("skip %s: %s" % (name, e))
            continue
        if fields is None:
            warn("skip %s: not an MS call" % name)
            continue
        if name in seen:
            warn("duplicate key %s; keeping first" % name)
            continue
        uf = fields.pop("unresolved_field", None)
        if uf is not None:
            warn("%s: non-literal %s; tagging LabelOnly" % (name, uf))
        seen.add(name)
        entries.append((name, fields))
    return entries


HEADER = """\
//! GENERATED by scripts/gen_modifier_settings.py for client build {build}.
//! Do not edit by hand. Re-run per game version. Modifier value-formatting
//! settings (the client MODIFIER_SETTINGS table), version-gated by build.
#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {{
{measure_variants}
}}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {{
    None,
    /// Take the absolute value before formatting (client `abs(value)` converter,
    /// e.g. newWaterline).
    Abs,
    /// Value is produced by a client getter/valueConverter that depends on
    /// per-ship runtime params (shipParams / GameParams / FieldParams) or an
    /// unresolved constant, so it cannot be ported as data; show the label only.
    LabelOnly,
}}

#[derive(Clone, Copy, Debug)]
pub struct ModifierSetting {{
    pub measure: Measure,
    pub base_value: f32,
    pub display_sign: bool,
    pub multiplier: f32,
    pub round_digits: u8,
    pub round_percents: bool,
    pub hidden: bool,
    pub measure_value_hidden: bool,
    pub positive: bool,
    pub transform: Transform,
    /// Client `translations` map: when the modifier value equals a key, the
    /// localized `IDS_*` label replaces the number entirely. Empty when absent.
    pub translations: &'static [(f32, &'static str)],
}}

impl Measure {{
    /// The `IDS_*` translation key for this measure's unit suffix, or `None`
    /// when the measure has no displayed unit.
    pub fn unit_ids_key(self) -> Option<&'static str> {{
        match self {{
{unit_arms}
        }}
    }}

    /// Whether a space separates the value from its unit. False only for
    /// percent (e.g. "+5%", but "5 km").
    pub fn space_before(self) -> bool {{
        !matches!(self, Measure::Percent)
    }}
}}

struct Table {{
    min_build: u32,
    entries: &'static [(&'static str, ModifierSetting)],
}}

/// The formatting settings for `name` at game `build`: the newest table with
/// `min_build <= build`, then the entry for `name`. `None` if no table covers
/// the build or the modifier is absent. Order-independent over TABLES.
pub fn modifier_setting(build: u32, name: &str) -> Option<&'static ModifierSetting> {{
    let mut chosen: Option<&Table> = None;
    for table in TABLES {{
        if table.min_build <= build
            && chosen.is_none_or(|c| table.min_build > c.min_build)
        {{
            chosen = Some(table);
        }}
    }}
    let table = chosen?;
    table
        .entries
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| s)
}}

"""


def emit(entries, build, used_variants):
    # Order: emit used variants (deterministic), always include None.
    order = list(MEASURE_VARIANTS.values())
    seen = set()
    variants = []
    for v in order:
        if (v in used_variants or v == "None") and v not in seen:
            seen.add(v)
            variants.append(v)
    measure_variants = "\n".join("    %s," % v for v in variants)

    unit_arms = []
    for v in variants:
        ids = MEASURE_IDS.get(v)
        if ids is None:
            unit_arms.append('            Measure::%s => None,' % v)
        else:
            unit_arms.append('            Measure::%s => Some("%s"),' % (v, ids))
    unit_arms = "\n".join(unit_arms)

    out = io.StringIO()
    out.write(HEADER.format(
        build=build,
        measure_variants=measure_variants,
        unit_arms=unit_arms,
    ))
    out.write("static TABLES: &[Table] = &[\n")
    out.write("    Table {\n")
    out.write("        min_build: %d,\n" % build)
    out.write("        entries: &[\n")
    for name, f in entries:
        out.write(
            '            ("%s", ModifierSetting { '
            'measure: Measure::%s, base_value: %s, display_sign: %s, '
            'multiplier: %s, round_digits: %d, round_percents: %s, '
            'hidden: %s, measure_value_hidden: %s, positive: %s, '
            'transform: Transform::%s, translations: %s }),\n'
            % (
                name,
                f["measure"],
                rust_f32(f["base_value"]),
                rust_bool(f["display_sign"]),
                rust_f32(f["multiplier"]),
                f["round_digits"],
                rust_bool(f["round_percents"]),
                rust_bool(f["hidden"]),
                rust_bool(f["measure_value_hidden"]),
                rust_bool(f["positive"]),
                f["transform"],
                rust_translations(f["translations"]),
            )
        )
    out.write("        ],\n")
    out.write("    },\n")
    out.write("];\n")
    out.write(FOOTER)
    return out.getvalue()


def rust_bool(b):
    return "true" if b else "false"


def rust_f32(x):
    if x == int(x):
        return "%d.0" % int(x)
    return repr(float(x))


def rust_translations(pairs):
    if not pairs:
        return "&[]"
    items = ", ".join('(%s, "%s")' % (rust_f32(k), ids) for k, ids in pairs)
    return "&[%s]" % items


FOOTER = r'''
/// Format the signed delta `val` for display: round to `round_digits` decimals,
/// but use 2 decimals when `abs(val) < 2.0` (the client's
/// MAXIMUM_ROUNDING_LIMIT_TO_TWO_DECIMAL_PLACES). Integral results show no
/// decimals; trailing zeros are stripped. The sign is the value's own: negatives
/// keep `-`, and when `display_sign` is set a `+` is forced for non-negative
/// values (the client `{:+}`/`{:-}` formats).
fn format_number(val: f32, round_digits: u8, display_sign: bool) -> String {
    let digits = if val.abs() < 2.0 { 2 } else { round_digits as usize };
    let factor = 10f32.powi(digits as i32);
    let mut rounded = (val * factor).round() / factor;
    // Force positive zero so a tiny negative delta never renders "-0".
    if rounded == 0.0 {
        rounded = 0.0;
    }
    let mut s = format!("{rounded:.digits$}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if display_sign && !s.starts_with('-') {
        format!("+{s}")
    } else {
        s
    }
}

/// Reproduce the client tooltip formatting for a single modifier: a description
/// fragment like "+10% Main battery reload time". `None` when the modifier is
/// hidden or its value equals the base (no change).
///
/// Label key order: `IDS_PARAMS_MODIFIER_<NAME>`, then the per-species suffix
/// `IDS_PARAMS_MODIFIER_<NAME>_<SPECIES>`, then the raw modifier name.
///
/// NOTE: `round_percents` is not yet honored; percent values always use the
/// `(v - base) * 100` path regardless of that flag.
pub fn format_modifier(
    build: u32,
    name: &str,
    value: f32,
    species: crate::game_params::types::Species,
    metadata: &dyn crate::data::ResourceLoader,
) -> Option<String> {
    let s = modifier_setting(build, name)?;
    if s.hidden {
        return None;
    }

    let label_only = matches!(s.transform, Transform::LabelOnly);
    // Apply the ported value transform before the base-value compare and the
    // percent/multiplier step; LabelOnly skips numbers entirely.
    let value = match s.transform {
        Transform::Abs => value.abs(),
        Transform::None | Transform::LabelOnly => value,
    };
    if !label_only && value == s.base_value {
        return None;
    }

    // Client `translations`: when the value equals a key, the localized label
    // replaces the number and unit entirely (ModifierSettings.localize).
    if let Some((_, ids)) = s.translations.iter().find(|(k, _)| *k == value) {
        return Some(
            metadata
                .localized_name_from_id(ids)
                .unwrap_or_else(|| ids.to_string()),
        );
    }

    let label = {
        let upper = name.to_uppercase();
        let species_upper = format!("{species:?}").to_uppercase();
        metadata
            .localized_name_from_id(&format!("IDS_PARAMS_MODIFIER_{upper}"))
            .or_else(|| {
                metadata.localized_name_from_id(&format!(
                    "IDS_PARAMS_MODIFIER_{upper}_{species_upper}"
                ))
            })
            .unwrap_or_else(|| name.to_string())
    };

    if label_only {
        return Some(label);
    }

    // `positive` is color-only (green/red category in the client) and is not
    // used here: the number's sign is the delta's own.
    let number = if s.measure_value_hidden {
        String::new()
    } else {
        let val = if s.measure == Measure::Percent {
            (value - s.base_value) * 100.0
        } else {
            value * s.multiplier
        };
        let unit = s
            .measure
            .unit_ids_key()
            .and_then(|key| metadata.localized_name_from_id(key))
            .unwrap_or_default();
        let num = format_number(val, s.round_digits, s.display_sign);
        if s.measure.space_before() {
            format!("{num} {unit}")
        } else {
            format!("{num}{unit}")
        }
    };

    if number.is_empty() {
        Some(label)
    } else {
        Some(format!("{number} {label}"))
    }
}

/// Format each `(name, value)` modifier for `species`, dropping the ones that
/// are hidden or unchanged.
pub fn describe_modifiers<'a>(
    build: u32,
    mods: impl IntoIterator<Item = (&'a str, f32)>,
    species: crate::game_params::types::Species,
    metadata: &dyn crate::data::ResourceLoader,
) -> Vec<String> {
    mods.into_iter()
        .filter_map(|(n, v)| format_modifier(build, n, v, species, metadata))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::Species;

    /// Echoes any id back so tests can assert value/unit exactly and that the
    /// label fragment carries the id.
    struct EchoLoader;
    impl crate::data::ResourceLoader for EchoLoader {
        fn localized_name_from_param(
            &self,
            _param: &crate::game_params::types::Param,
        ) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &str) -> Option<String> {
            Some(id.to_string())
        }
        fn game_param_by_id(
            &self,
            _id: crate::game_types::GameParamId,
        ) -> Option<crate::Rc<crate::game_params::types::Param>> {
            None
        }
        fn entity_specs(&self) -> &[crate::rpc::entitydefs::EntitySpec] {
            &[]
        }
    }

    const BUILD: u32 = 11791718;

    #[test]
    fn format_number_negative_zero_renders_positive() {
        // A tiny negative delta rounds to zero; it must render "0", never "-0".
        let plain = format_number(-0.0001, 2, false);
        assert_eq!(plain, "0", "got {plain}");
        assert!(!plain.contains("-0"), "got {plain}");
        let signed = format_number(-0.0001, 2, true);
        assert_eq!(signed, "+0", "got {signed}");
        assert!(!signed.contains("-0"), "got {signed}");
    }

    #[test]
    fn percent_negative_delta_natural_sign() {
        // GMRotationSpeed: percent, base 1.0. delta (0.9-1.0)*100 = -10 => "-".
        let out = format_modifier(BUILD, "GMRotationSpeed", 0.9, Species::Battleship, &EchoLoader)
            .expect("should render");
        assert!(out.starts_with("-10IDS_PERCENT "), "got {out}");
        assert!(out.contains("IDS_PARAMS_MODIFIER_GMROTATIONSPEED"), "got {out}");
    }

    #[test]
    fn percent_negative_delta_follows_value_not_positive_flag() {
        // GMShotDelay: percent, base 1.0, positive=false. delta -10 => "-10",
        // not "+10": the sign follows the delta, not the value-type flag.
        let out = format_modifier(BUILD, "GMShotDelay", 0.9, Species::Battleship, &EchoLoader)
            .expect("should render");
        assert!(out.starts_with("-10IDS_PERCENT "), "got {out}");
    }

    #[test]
    fn percent_positive_delta_with_display_sign() {
        // GMRotationSpeed: display_sign=true. delta (1.1-1.0)*100 = +10 => "+".
        let out = format_modifier(BUILD, "GMRotationSpeed", 1.1, Species::Battleship, &EchoLoader)
            .expect("should render");
        assert!(out.starts_with("+10IDS_PERCENT "), "got {out}");
    }

    #[test]
    fn abs_transform_uses_absolute_value() {
        // newWaterline: Transform::Abs (client `abs(value)`), Meter, base 0.0,
        // display_sign=false. A negative raw value renders its magnitude.
        let out = format_modifier(BUILD, "newWaterline", -5.0, Species::Submarine, &EchoLoader)
            .expect("abs of -5 differs from base 0, should render");
        assert!(out.starts_with("5 IDS_METER "), "got {out}");
        assert!(!out.contains("-5"), "got {out}");
        assert!(out.contains("IDS_PARAMS_MODIFIER_NEWWATERLINE"), "got {out}");
    }

    #[test]
    fn label_only_renders_label_without_number() {
        // patrolPlaneCount: Transform::LabelOnly. Renders the label id only, no
        // digits or unit, and stays Some even when value equals the base.
        let out = format_modifier(BUILD, "patrolPlaneCount", 0.0, Species::AirCarrier, &EchoLoader)
            .expect("LabelOnly should render even when value == base");
        assert_eq!(out, "IDS_PARAMS_MODIFIER_PATROLPLANECOUNT", "got {out}");
        assert!(!out.chars().any(|c| c.is_ascii_digit()), "got {out}");
    }

    #[test]
    fn value_equal_base_is_none() {
        assert!(
            format_modifier(BUILD, "GMShotDelay", 1.0, Species::Battleship, &EchoLoader).is_none()
        );
    }

    #[test]
    fn hidden_modifier_is_none() {
        // artilleryAlertMinDistance is hidden.
        assert!(
            format_modifier(BUILD, "artilleryAlertMinDistance", 5.0, Species::Battleship, &EchoLoader)
                .is_none()
        );
    }

    #[test]
    fn describe_filters_unchanged_and_hidden() {
        let out = describe_modifiers(
            BUILD,
            [
                ("GMRotationSpeed", 0.9),
                ("GMShotDelay", 1.0),
                ("artilleryAlertMinDistance", 5.0),
            ],
            Species::Battleship,
            &EchoLoader,
        );
        assert_eq!(out.len(), 1, "got {out:?}");
    }

    #[test]
    fn lookup_is_version_gated() {
        assert!(modifier_setting(11791718, "shootShift").is_some());
        assert!(modifier_setting(1, "shootShift").is_none());
        assert!(modifier_setting(11791718, "definitely_not_a_modifier").is_none());
    }

    #[test]
    fn measure_units() {
        assert_eq!(Measure::Percent.unit_ids_key(), Some("IDS_PERCENT"));
        assert!(!Measure::Percent.space_before());
        assert_eq!(Measure::None.unit_ids_key(), None);
    }

    #[test]
    fn consumable_effect_fields_have_settings() {
        // Consumable tooltips reuse MODIFIER_SETTINGS for these fields; the
        // Describable API for Ability depends on their presence at this build.
        for field in ["regenerationHPSpeed", "regenerationHPSpeedUnits", "radius", "smokeGeneratorLifeTime", "workTime", "reloadTime", "preparationTime"] {
            assert!(modifier_setting(BUILD, field).is_some(), "missing setting for {field}");
        }
    }

    #[test]
    fn infinity_translation_replaces_negative_one() {
        // numConsumables carries translations={-1.0: ..._INFINITY_NUM}: the -1
        // "unlimited" sentinel renders the localized infinity label, not "-1".
        let out = format_modifier(BUILD, "numConsumables", -1.0, Species::Battleship, &EchoLoader)
            .expect("infinity sentinel should render the translated label");
        assert_eq!(out, "IDS_PARAMS_MODIFIER_CONSUMABLES_INFINITY_NUM", "got {out}");
        assert!(!out.contains("-1"), "got {out}");

        // A normal count (!= base 0.0) still renders numerically, never the
        // infinity label.
        let normal = format_modifier(BUILD, "numConsumables", 3.0, Species::Battleship, &EchoLoader)
            .expect("a normal count should render");
        assert!(normal.contains('3'), "got {normal}");
        assert!(!normal.contains("INFINITY"), "got {normal}");
    }
}
'''


def main():
    ms_path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_MS
    mvt_path = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_MVT
    build = int(sys.argv[3]) if len(sys.argv) > 3 else DEFAULT_BUILD

    mvt_map = build_mvt_map(mvt_path)
    sys.stderr.write("MVT registry: %d entries\n" % len(mvt_map))

    stats = {"label_only": 0, "mvt_unresolved": 0, "unresolved_value": 0}
    entries = extract_entries(ms_path, mvt_map, stats)

    used = set(f["measure"] for _, f in entries)
    rust = emit(entries, build, used)

    with io.open(RS_OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(rust)

    sys.stderr.write("emitted %d entries to %s\n" % (len(entries), RS_OUT))
    sys.stderr.write("Transform::LabelOnly (getter/valueConverter): %d\n" % stats["label_only"])
    sys.stderr.write("Transform::LabelOnly (unresolved_value): %d\n" % stats["unresolved_value"])
    sys.stderr.write("MVT isPositive unresolved (defaulted True): %d\n" % stats["mvt_unresolved"])


if __name__ == "__main__":
    main()
