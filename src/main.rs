use clap::{App, Arg, ArgGroup, ArgMatches};
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

#[derive(Debug, Copy, Clone)]
enum Token {
    Add(i32), AddOffset(i32, i32), Zero, ZeroOffset(i32), Move(i32), In, Out, BeginLoop, EndLoop, Null, Mult(i32, i32)//, Debug(i32)
}

struct Optimisations {
    zero_loop: bool,
    clump_ops: bool,
    move_loop: bool,
    copy_loop: bool,
    mult_loop: bool,
    add_offset: bool,
    zero_offset: bool
}

fn main() {
    let args = match_args();
    let input_path = Path::new(args.value_of("input").unwrap());
    let output_path = Path::new(args.value_of("output").unwrap());
    let mut input_file = File::open(input_path).unwrap();
    let mut input_contents = String::new();
    input_file.read_to_string(&mut input_contents).unwrap();

    let mut output_file = File::create(output_path).unwrap();
        
    // Datatype to use for tape: int8_t, int16_t, int32_t, int64_t
    let bitcount = args.value_of("bits").unwrap_or("8");
    let datatype = match bitcount {
        "8" | "16" | "32" | "64" => format!("int{}_t", bitcount),
        _ => panic!("Error: bit count must be 8, 16, 32, or 64")
    };
    
    // EOL handling depending on flags
    let inputcode = match (args.is_present("eof-zero"), args.is_present("eof-neg-one"), args.is_present("eof-unchanged")) {
        (false, false, false) 
            => "*ptr=getchar();".to_string(),
        (true, false, false) 
            => format!("inbuf=getchar();*ptr=(inbuf==({})(EOF))?0:inbuf;", datatype),
        (false, true, false) 
            => format!("inbuf=getchar();*ptr=(inbuf==({})(EOF))?-1:inbuf;", datatype),
        (false, false, true) 
            => format!("inbuf=getchar();*ptr=(inbuf==({})(EOF))?*ptr:inbuf;", datatype),
        _ => panic!()
    };

    // Length of tape (defaults to 30000)
    let tapelength = args.value_of("length").unwrap_or("30000");

    let optilevel: u8 = args.value_of("optimize").unwrap_or("3").parse().unwrap();

    // Optimisations to apply
    let mut optimizations = Optimisations {
        zero_loop: false,
        clump_ops: false,
        move_loop: false,
        copy_loop: false,
        mult_loop: false,
        add_offset: false,
        zero_offset: false
    };

    if optilevel >= 1 {
        optimizations.zero_loop = true;
        optimizations.clump_ops = true;
    }
    if optilevel >= 2 {
        optimizations.move_loop = true;
        optimizations.copy_loop = true;
        optimizations.mult_loop = true;
    }
    if optilevel >= 3 {
        optimizations.add_offset = true;
        optimizations.zero_offset = true;
    }
    
    // Ignore any invalid characters
    let mut code: String = input_contents.chars().into_iter().filter(|c| match c {
        '+' | '-' | '<' | '>' | '[' | ']' | ',' | '.' => true,
        _ => false
    }).collect::<String>();
    
    // [-] and [+] set cell to 0
    if optimizations.zero_loop {
        code = code.replace("[-]", "0").replace("[+]", "0");
    }

    // Tokenization and grouping of +++, ---, <<<, >>>
    let mut tokens = Vec::<Token>::new();
    let mut current = Token::Null;
    let clump = optimizations.clump_ops;
    for c in code.chars() {
        match (c, current) {
            // Clumping rules
            ('+', Token::Add(x)) if clump => current = Token::Add(x + 1),
            ('-', Token::Add(x)) if clump => current = Token::Add(x - 1),
            ('0', Token::Add(_)) if clump => current = Token::Zero,
            ('0', Token::Zero) if clump => (),
            ('>', Token::Move(x)) if clump => current = Token::Move(x + 1),
            ('<', Token::Move(x)) if clump => current = Token::Move(x - 1),
            // Normal rules
            ('+', _) => {tokens.push(current); current = Token::Add(1);},
            ('-', _) => {tokens.push(current); current = Token::Add(-1);},
            ('0', _) => {tokens.push(current); current = Token::Zero;},
            ('>', _) => {tokens.push(current); current = Token::Move(1);},
            ('<', _) => {tokens.push(current); current = Token::Move(-1);}
            ('.', _) => {tokens.push(current); current = Token::Out;}
            (',', _) => {tokens.push(current); current = Token::In;}
            ('[', _) => {tokens.push(current); current = Token::BeginLoop;}
            (']', _) => {tokens.push(current); current = Token::EndLoop;}
            (_,_) => ()
        }
    }
    tokens.push(current);
    // Remove invalid tokens
    tokens = tokens.into_iter().filter(|t| match t {
        Token::Null | Token::Add(0) | Token::Move(0) => false,
        _ => true
    }).collect::<Vec<Token>>();

    // Optimisations:
    // Replace copy and mult loops with copy/clear instructions
    // Replace move,add,move with offset add
    let mut newtokens = Vec::<Token>::new();
    let mut i: usize = 0;
    for _ in 0..8 {
        tokens.push(Token::Null);
    }
    while i < tokens.len() - 8 {
        match &tokens[i..(i+8)] {
            [
                Token::BeginLoop, Token::Add(-1),
                Token::Move(m1), Token::Add(a1),
                Token::Move(m2), Token::EndLoop,_,_
            ] | [
                Token::BeginLoop,
                Token::Move(m1), Token::Add(a1),
                Token::Move(m2), Token::Add(-1),
                Token::EndLoop,_,_
            ] if *m1 == -*m2 && optimizations.move_loop => {
                newtokens.push(Token::Mult(*m1, *a1));
                newtokens.push(Token::Zero);
                i += 5;
            },
            [
                Token::BeginLoop, Token::Add(-1), 
                Token::Move(m1), Token::Add(a1), 
                Token::Move(m2), Token::Add(a2), 
                Token::Move(m3), Token::EndLoop
            ] | [
                Token::BeginLoop, 
                Token::Move(m1), Token::Add(a1), 
                Token::Move(m2), Token::Add(a2), 
                Token::Move(m3), Token::Add(-1),
                Token::EndLoop
            ] if -m3 == m1 + m2 && optimizations.copy_loop &&
                ((*a1 == 1 && *a2 == 1) || optimizations.mult_loop) => {
                newtokens.push(Token::Mult(*m1, *a1));
                newtokens.push(Token::Mult(*m1+*m2, *a2));
                newtokens.push(Token::Zero);
                i += 7;
            },
            [
                Token::Move(m1), Token::Add(a), Token::Move(m2),
                _,_,_,_,_
            ] if ((*m1 > 0 && *m2 < 0) || (*m1 < 0 && *m2 > 0)) &&
                optimizations.add_offset => {
                if *m1 == -*m2 {
                    newtokens.push(Token::AddOffset(*a, *m1));
                } else {
                    newtokens.push(Token::AddOffset(*a, *m1));
                    newtokens.push(Token::Move(*m1+*m2));
                }
                i += 2;
            },
            [
                Token::Move(m1), Token::Zero, Token::Move(m2),
                _,_,_,_,_
            ] if ((*m1 > 0 && *m2 < 0) || (*m1 < 0 && *m2 > 0)) &&
                optimizations.zero_offset => {
                if *m1 == -*m2 {
                    newtokens.push(Token::ZeroOffset(*m1));
                } else {
                    newtokens.push(Token::ZeroOffset(*m1));
                    newtokens.push(Token::Move(*m1+*m2));
                }
                i += 2;
            },_ => newtokens.push(tokens[i])
        }
        i += 1
    }

    // Code generation
    output_file.write(b"#include <stdio.h>\n#include <inttypes.h>\n").unwrap();
    output_file.write(format!(
            "{type} mem[{len}]; {type} *ptr = mem;\n{type} inbuf;\n", type=datatype, len=tapelength).as_bytes()).unwrap();
    output_file.write(b"int main() {").unwrap();

    for token in newtokens {
        output_file.write(match token {
            Token::Add(x) => format!("*ptr+={};\n", x),
            Token::AddOffset(x, o) => format!("*(ptr+{})+={};\n", o, x),
            Token::Move(x) => format!("ptr+={};\n", x),
            Token::Zero => "*ptr=0;\n".to_string(),
            Token::ZeroOffset(x) => format!("*(ptr+{})=0;\n", x),
            Token::Mult(x, y) => format!("*(ptr+{})+=*ptr*{};\n", x, y).to_string(),
            Token::BeginLoop => "while(*ptr){\n".to_string(),
            Token::EndLoop => "}\n".to_string(),
            Token::Out => "putchar(*ptr);\n".to_string(),
            Token::In => inputcode.to_string(),
            //Token::Debug(x) => format!("printf(\"DEBUG{}\\n\");\n", x),
            _ => panic!()
        }.as_bytes()).unwrap();
    }
    output_file.write(b"return 0;\n}\n").unwrap();
}

fn match_args() -> ArgMatches<'static> {
    App::new("tribf")
        .version("1.0")
        .about("Optimises and transpiles Brainfuck code to C")
        .arg(Arg::with_name("input")
             .required(true)
             .index(1)
             .help("Name of the input file"))
        .arg(Arg::with_name("output")
             .short("o").long("output")
             .takes_value(true)
             .default_value("o.c")
             .help("Name of the output file"))
        .arg(Arg::with_name("length")
             .short("l").long("length")
             .takes_value(true)
             .default_value("30000")
             .help("Length of the tape"))
        .arg(Arg::with_name("bits")
             .short("b").long("bits")
             .takes_value(true)
             .default_value("8")
             .help("Bit count of each cell (must be 8, 16, 32, or 64)"))
        .arg(Arg::with_name("eof-zero")
             .short("z").long("eof-zero")
             .takes_value(false)
             .help("EOFs write a 0"))
        .arg(Arg::with_name("eof-neg-one")
             .short("n").long("eof-neg-one")
             .takes_value(false)
             .help("EOFs write a -1"))
        .arg(Arg::with_name("eof-unchanged")
             .short("u").long("eof-unchanged")
             .takes_value(false)
             .help("EOFs leave the cell unchanged"))
        .arg(Arg::with_name("optimize")
             .short("O").long("optimize")
             .takes_value(true)
             .help("Set optimization level (0, 1, 2, 3)"))
        .group(ArgGroup::with_name("eof-handling")
               .args(&["eof-zero", "eof-neg-one", "eof-unchanged"])
               .required(false))
        .get_matches()
}
