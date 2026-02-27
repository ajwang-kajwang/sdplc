mod lexer;
use lexer::Lexer;
use lexer::TokenType;

fn main() {
    // 1. Dummy IEC 61131-3 source code
    let source_code = "PROGRAM MyFirstPLC VAR count : INT := 0; END_VAR";

    println!("Scanning Source: \"{}\"\n", source_code);

    // 2. Initialize the Lexer
    let mut lexer = Lexer::new(source_code);

    // 3. Loop through all tokens until EOF
    loop {
        let token = lexer.next_token();
        
        println!("Token: {:?} | Text: '{}'", token.kind, token.text);

        if token.kind == TokenType::Eof {
            break;
        }
    }
}