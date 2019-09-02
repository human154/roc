use region;
use operator::Operator;
use typed_arena::Arena;
use std::mem;

// Strategy:
//
// 1. Let space parsers check indentation. They should expect indentation to only ever increase (right?) when
//    doing a many_whitespaces or many1_whitespaces. Multline strings can have separate whitespace parsers.
// 2. For any expression that has subexpressions (e.g. ifs, parens, operators) record their indentation levels
//    by doing .and(position()) followed by .and_then() which says "I can have a declaration inside me as
//    long as the entire decl is indented more than me."
// 3. Make an alternative to RangeStreamOnce where uncons_while barfs on \t (or maybe just do this in whitespaces?)

type Loc<T> = region::Located<T>;

/// Struct which represents a position in a source file.
#[derive(Debug, Clone)]
pub struct State<'a> {
    /// The raw input string.
    pub input: &'a str,

    /// Current line of the input
    pub line: u32,
    /// Current column of the input
    pub column: u32,

    /// Current indentation level, in columns 
    /// (so no indent is col 1 - this saves an arithmetic operation.)
    pub indent_col: u32,

    // true at the beginning of each line, then false after encountering 
    // the first nonspace char on that line.
    pub is_indenting: bool,
}

/// The length of a short slice. This lets us store certain strings inline
/// without having to allocate them on the heat. The number is calibrated to be 
/// as high as possible without causing Expr's memory footprint to increase.
///
/// It is calculated this way:
///
/// 1. Expr needs 2 machine words to store its largest variant.
/// 2. It also needs a 1-byte tag, but memory alignment expands that to a word.
/// 3. Since that word is all padding except for 1 byte, we can use n-1 bytes.
const SHORT_SLICE_LEN: usize = 
    (mem::size_of::<usize>() * 3) - 1; // 23 on 64-bit systems; 11 on 32-bit

type Ident = str;
type VariantName = str;

/// A parsed expression. This uses lifetimes extensively for two reasons:
///
/// 1. It uses Arena::alloc for all allocations, which returns a reference.
/// 2. It often stores references into the input string instead of allocating.
///
/// This dramatically reduces allocations during parsing. Once parsing is done,
/// we move on to canonicalization, which often needs to allocate more because
/// it's doing things like turning local variables into fully qualified symbols.
/// Once canonicalization is done, the arena and the input string get dropped.
///
/// Because we need to store references, which each take 2 machine words, the
/// smallest this data structure can be in memory is 3 machine words (the third
/// machine word stores the 1-byte union tag in a memory-aligned way). We have
/// a test verifying that it never accidentally exceeds 3 machine words in size.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr<'a> {
    // Number Literals
    Int(i64),
    Float(f64),
    
    // String Literals
    EmptyStr,
    ShortStr([u8; SHORT_SLICE_LEN]),
    LongStr(&'a str),
    /// basically InterpolatedStr(Vec<(String, Loc<Ident>)>, String)
    InterpolatedStr(&'a (&'a [(&'a str, Loc<&'a Ident>)], &'a str)),

    // List literals
    EmptyList,
    List(&'a [Loc<Expr<'a>>]),

    // Lookups
    ShortVar([u8; SHORT_SLICE_LEN]),
    LongVar(&'a Ident),

    // Pattern Matching
    Case(&'a (Loc<Expr<'a>>, [(Loc<Pattern<'a>>, Loc<Expr<'a>>)])),
    Closure(&'a (&'a [Loc<Pattern<'a>>], Loc<Expr<'a>>)),
    /// basically Assign(Vec<(Loc<Pattern>, Loc<Expr>)>, Loc<Expr>)
    Assign(&'a (&'a [(Loc<Pattern<'a>>, Loc<Expr<'a>>)], Loc<Expr<'a>>)),

    // Application
    Call(&'a (Loc<Expr<'a>>, [Loc<Expr<'a>>])),
    ApplyVariant(&'a (&'a VariantName, [Loc<Expr<'a>>])),
    Variant(&'a VariantName),

    // Product Types
    EmptyRecord,

    // Sugar
    If(&'a (Loc<Expr<'a>>, Loc<Expr<'a>>, Loc<Expr<'a>>)),
    Operator(&'a (Loc<Expr<'a>>, Loc<Operator>, Loc<Expr<'a>>)),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pattern<'a> {
    // Identifier
    ShortIdentifier([u8; SHORT_SLICE_LEN]),
    LongIdentifier(&'a Ident),

    // Variant
    ShortVariant([u8; SHORT_SLICE_LEN]),
    LongVariant(&'a VariantName),
    AppliedVariant(&'a (Loc<&'a VariantName>, [Loc<Pattern<'a>>])),

    // Literal
    IntLiteral(i64),
    FloatLiteral(f64),
    ShortStringLiteral([u8; SHORT_SLICE_LEN]),
    LongStringLiteral(&'a str),
    EmptyRecordLiteral,
    Underscore,
}


#[derive(Clone, Debug, PartialEq)]
pub enum CanExpr {
    // Literals
    Int(i64),
    Float(f64),
    EmptyStr,
    Str(Box<str>),
    Char(char),
    List(Vec<Loc<CanExpr>>),
    EmptyList,
    EmptyRecord,
}

// fn _canonicalize<'a>(raw: &'a str, expr: Expr<'a>) -> CanExpr {
//     use self::CanExpr::*;

//     match expr {
//         Expr::Int(num) => Int(num),
//         Expr::Float(num) => Float(num), 
//         Expr::EmptyRecord => EmptyRecord,
//         Expr::ShortStr(bytes) => {
//             let boxed: Box<str> = unsafe {
//                 // This is safe because these bytes were read directly out
//                 // of a utf-8 string, along appropriate code point boundaries.
//                 std::str::from_utf8_unchecked(&bytes)
//             }.into();

//             Str(boxed)
//         },
//         Expr::MedStr(offset, len) => {
//             let boxed: Box<str> = raw[offset..(offset + len as usize)].into();

//             Str(boxed)
//         }
//         Expr::LongStr(boxed_str) => Str((*boxed_str).into()),
//         Expr::EmptyStr => EmptyStr,
//         Expr::EmptyList => EmptyList,
//         _ => panic!("disco")
//     }
// }


#[test]
fn expr_size() {
    // The size of the Expr data structure should be exactly 3 machine words.
    // This test helps avoid regressions wich accidentally increase its size!
    assert_eq!(
        std::mem::size_of::<Expr>(),
        std::mem::size_of::<usize>() * 3
    );
}

#[test]
fn pattern_size() {
    // The size of the Pattern data structure should be exactly 3 machine words.
    // This test helps avoid regressions wich accidentally increase its size!
    assert_eq!(
        std::mem::size_of::<Pattern>(),
        std::mem::size_of::<usize>() * 3
    );
}


type ParseResult<'a, Output> = Result<(State<'a>, Output), State<'a>>;

struct Env<'a> {
    expr_allocator: Arena<Expr<'a>>, 
    pattern_allocator: Arena<Pattern<'a>>, 
    state: State<'a>,
}

trait Parser<'a, Output> {
    fn parse(&self, &'a Env<'a>) -> ParseResult<'a, Output>;
}


impl<'a, F, Output> Parser<'a, Output> for F
where F: Fn(&'a Env<'a>) -> ParseResult<'a, Output>,
{
    fn parse(&self, env: &'a Env<'a>) -> ParseResult<'a, Output> {
        self(env)
    }
}

fn map<'a, P, F, Before, After>(parser: P, transform: F) -> impl Parser<'a, After>
where
    P: Parser<'a, Before>,
    F: Fn(Before) -> After,
{
    move |env|
        parser
            .parse(env)
            .map(|(next_state, output)| (next_state, transform(output)))
}

/// A keyword with no newlines in it.
fn keyword<'a>(kw: &'static str) -> impl Parser<'a, ()> {
    // We can't have newlines because we don't attempt to advance the row
    // in the state, only the column.
    debug_assert!(!kw.contains("\n"));

    move |env: &'a Env| {
        let input = env.state.input;

        match input.get(0..kw.len()) {
            Some(next) if next == kw => {
                let len = kw.len();

                Ok((State {
                    input: &input[len..],
                    column: env.state.column + len as u32,
                    
                    ..env.state
                }, ()))
            },
            _ => Err(env.state.clone()),
        }
    }
}

fn satisfies<'a, P, A, F>(parser: P, predicate: F) -> impl Parser<'a, A>
where
    P: Parser<'a, A>,
    F: Fn(&A) -> bool,
{
    move |env| {
        if let Ok((next_state, output)) = parser.parse(env) {
            if predicate(&output) {
                return Ok((next_state, output));
            }
        }

        Err(env.state.clone())
    }
}

fn any<'a>(env: &'a Env) -> ParseResult<'a, char> {
    let input = env.state.input;

    match input.chars().next() {
        Some(ch) => {
            let len = ch.len_utf8();
            let mut new_state = State {
                input: &input[len..],
                
                ..env.state
            };

            if ch == '\n' {
                new_state.line = new_state.line + 1;
                new_state.column = 0;
            }

            Ok((new_state, ch))
        }
        _ => Err(env.state.clone()),
    }
}

fn whitespace<'a>() -> impl Parser<'a, char> {
    satisfies(any, |ch| ch.is_whitespace())
}


/// What we're currently attempting to parse, e.g. 
/// "currently attempting to parse a list." This helps error messages!
#[derive(Debug, Clone, Copy)]
pub enum Attempting {
    List,
    Keyword,
}

