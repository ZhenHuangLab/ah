//! TeX math as Unicode text for the terminal: Greek letters, operators, simple sub- and
//! superscripts, fractions and font commands. Anything unknown is kept as written.

pub fn unicode(tex: &str) -> String {
    let cs: Vec<char> = tex.chars().collect();
    let mut i = 0;
    seq(&cs, &mut i, false).trim().to_string()
}

fn seq(cs: &[char], i: &mut usize, group: bool) -> String {
    let mut out = String::new();
    while *i < cs.len() {
        let c = cs[*i];
        *i += 1;
        match c {
            '}' if group => return out,
            '{' => out.push_str(&seq(cs, i, true)),
            '\\' => out.push_str(&command(cs, i)),
            '^' | '_' => {
                let a = arg(cs, i);
                out.push_str(&script(&a, c == '^'));
            }
            '~' | '&' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A command argument: a braced group or a single token.
fn arg(cs: &[char], i: &mut usize) -> String {
    while *i < cs.len() && cs[*i] == ' ' {
        *i += 1;
    }
    let Some(&c) = cs.get(*i) else { return String::new() };
    *i += 1;
    match c {
        '{' => seq(cs, i, true),
        '\\' => command(cs, i),
        c => c.to_string(),
    }
}

fn command(cs: &[char], i: &mut usize) -> String {
    let start = *i;
    match cs.get(*i) {
        None => return "\\".into(),
        Some(c) if c.is_ascii_alphabetic() => {
            while cs.get(*i).is_some_and(|c| c.is_ascii_alphabetic()) {
                *i += 1;
            }
        }
        Some(_) => *i += 1,
    }
    let name: String = cs[start..*i].iter().collect();
    let s = match name.as_str() {
        "," | ";" | ":" | " " | "quad" => " ",
        "qquad" => "  ",
        "!" | "displaystyle" | "textstyle" | "limits" | "nolimits" => "",
        "\\" => "\n",
        "|" => "‖",
        "{" | "}" | "_" | "%" | "$" | "#" | "&" => return name,
        "left" | "right" | "middle" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" => {
            if cs.get(*i) == Some(&'.') {
                *i += 1;
            }
            ""
        }
        "frac" | "dfrac" | "tfrac" => {
            let (a, b) = (arg(cs, i), arg(cs, i));
            return format!("{}/{}", wrap(&a), wrap(&b));
        }
        "sqrt" => {
            if cs.get(*i) == Some(&'[') {
                while *i < cs.len() && cs[*i] != ']' {
                    *i += 1;
                }
                *i += 1;
            }
            return format!("√{}", wrap(&arg(cs, i)));
        }
        "binom" => {
            let (a, b) = (arg(cs, i), arg(cs, i));
            return format!("C({a}, {b})");
        }
        "begin" | "end" => {
            arg(cs, i);
            ""
        }
        "text" | "textrm" | "textbf" | "textit" | "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "boldsymbol" | "bm"
        | "operatorname" | "mathcal" | "mathscr" | "mathfrak" | "emph" => return arg(cs, i),
        "mathbb" => return arg(cs, i).chars().map(double_struck).collect(),
        f if FUNCTIONS.contains(&f) => {
            let before = if start >= 2 && cs[start - 2].is_alphanumeric() { " " } else { "" };
            let after = if cs.get(*i).is_some_and(|c| c.is_alphanumeric() || *c == '\\') { " " } else { "" };
            return format!("{before}{name}{after}");
        }
        "hat" | "widehat" => return accent(arg(cs, i), '\u{302}'),
        "tilde" | "widetilde" => return accent(arg(cs, i), '\u{303}'),
        "bar" | "overline" => return accent(arg(cs, i), '\u{304}'),
        "vec" => return accent(arg(cs, i), '\u{20d7}'),
        "dot" => return accent(arg(cs, i), '\u{307}'),
        "ddot" => return accent(arg(cs, i), '\u{308}'),
        _ => match symbol(&name) {
            Some(s) => s,
            None => return format!("\\{name}"),
        },
    };
    s.to_string()
}

/// Parenthesizes anything longer than a single term.
fn wrap(s: &str) -> String {
    if s.chars().all(|c| c.is_alphanumeric() || SUP.contains(&c) || SUB.contains(&c)) { s.to_string() } else { format!("({s})") }
}

fn accent(s: String, mark: char) -> String {
    let mut s = s;
    if !s.is_empty() {
        s.push(mark);
    }
    s
}

/// Operator names that TeX sets upright; shown as the bare word.
const FUNCTIONS: &[&str] = &[
    "lim", "max", "min", "sup", "inf", "arg", "det", "exp", "log", "ln", "sin", "cos", "tan", "sinh", "cosh", "tanh", "arcsin", "arccos",
    "arctan", "Pr", "gcd", "deg", "dim", "ker", "tr", "argmin", "argmax",
];

const PLAIN: &str = "0123456789+-=()";
const SUP: [char; 15] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹', '⁺', '⁻', '⁼', '⁽', '⁾'];
const SUB: [char; 15] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉', '₊', '₋', '₌', '₍', '₎'];

fn sup(c: char) -> Option<char> {
    if let Some(k) = PLAIN.find(c) {
        return Some(SUP[k]);
    }
    Some(match c {
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'T' => 'ᵀ',
        '*' | '′' | '†' => c,
        '\'' => '′',
        _ => return None,
    })
}

fn sub(c: char) -> Option<char> {
    if let Some(k) = PLAIN.find(c) {
        return Some(SUB[k]);
    }
    Some(match c {
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        'β' => 'ᵦ',
        'γ' => 'ᵧ',
        'ρ' => 'ᵨ',
        'φ' | 'ϕ' => 'ᵩ',
        'χ' => 'ᵪ',
        _ => return None,
    })
}

/// `x^{…}` / `x_{…}` with Unicode script characters when every character has one.
fn script(a: &str, up: bool) -> String {
    let a = a.trim();
    let map = if up { sup } else { sub };
    if let Some(s) = a.chars().filter(|c| *c != ' ').map(map).collect::<Option<String>>() {
        return s;
    }
    let mark = if up { '^' } else { '_' };
    if a.chars().count() == 1 { format!("{mark}{a}") } else { format!("{mark}({a})") }
}

fn double_struck(c: char) -> char {
    match c {
        'R' => 'ℝ',
        'N' => 'ℕ',
        'Z' => 'ℤ',
        'Q' => 'ℚ',
        'C' => 'ℂ',
        'P' => 'ℙ',
        'H' => 'ℍ',
        'E' => '𝔼',
        '1' => '𝟙',
        c => c,
    }
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "cdot" => "·",
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "ll" => "≪",
        "gg" => "≫",
        "lesssim" => "≲",
        "gtrsim" => "≳",
        "approx" => "≈",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "equiv" => "≡",
        "propto" => "∝",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "iint" => "∬",
        "oint" => "∮",
        "to" | "rightarrow" => "→",
        "leftarrow" | "gets" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "mapsto" => "↦",
        "longrightarrow" => "⟶",
        "longleftarrow" => "⟵",
        "longleftrightarrow" => "⟷",
        "Longrightarrow" | "implies" => "⟹",
        "Longleftarrow" | "impliedby" => "⟸",
        "Longleftrightarrow" | "iff" => "⟺",
        "longmapsto" => "⟼",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "setminus" => "∖",
        "emptyset" | "varnothing" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "neg" | "lnot" => "¬",
        "land" | "wedge" => "∧",
        "lor" | "vee" => "∨",
        "oplus" => "⊕",
        "otimes" => "⊗",
        "circ" => "∘",
        "bullet" => "•",
        "star" => "⋆",
        "ast" => "∗",
        "dagger" => "†",
        "ldots" | "dots" | "cdots" | "dotsc" | "dotsb" => "…",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "Vert" | "lVert" | "rVert" => "‖",
        "vert" | "lvert" | "rvert" | "mid" => "|",
        "perp" => "⊥",
        "parallel" => "∥",
        "angle" => "∠",
        "prime" => "′",
        "hbar" => "ℏ",
        "ell" => "ℓ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "degree" => "°",
        _ => return None,
    })
}
