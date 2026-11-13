//! Checked borrows execute against the original target storage, including aliases.
use super::{execute, parse};

#[test]
fn rustc_resource_references_can_be_reselected_without_moving_owners() {
    execute(
        "resource-reference-selection",
        r#"
mod owner;use owner::*;
fn main(){
 let a=Renderer::new(4);let b=Renderer::new(9);
 let mut r=&a;if r.identity()!=4{halt(0xbad);}
 r=&b;if r.identity()!=9{halt(0xbad);}
 halt(0x600d);
}
"#,
    );
}

#[test]
fn rustc_value_methods_can_borrow_resources_and_return_resource_owners() {
    execute(
        "value-method-resources",
        r#"
mod owner;use owner::*;
#[derive(Copy,Clone)]struct Settings{offset:u16}
impl Settings{
 fn read(&self,r:&Renderer)->u16{self.offset+r.identity()}
 fn pair(self,a:&Renderer,b:&Renderer)->u16{self.offset+shared_pair(a,b)}
 fn create(&self)->Renderer{Renderer::new(self.offset)}
}
fn main(){
 let settings=Settings{offset:3};let a=Renderer::new(4);let b=Renderer::new(9);
 if settings.read(&a)!=7||settings.pair(&a,&b)!=16{halt(0xbad);}
 let borrowed=&settings;if borrowed.read(&b)!=12{halt(0xbad);}
 let created=borrowed.create();if created.identity()!=3{halt(0xbad);}
 halt(0x600d);
}
"#,
    );
}

#[test]
fn rustc_ordinary_references_preserve_original_places_and_reborrows() {
    execute(
        "ordinary-references",
        r#"
#[derive(Copy,Clone)]struct Pair{left:u16,right:u16}
struct Nested{pair:Pair}
#[repr(u16)]#[derive(Copy,Clone)]enum Mode{A,B}
impl Mode{fn same(&self,other:&Self)->bool{*self as u16==*other as u16}}
impl Pair{fn sum(&self)->u16{self.left+self.right} fn bump(&mut self){self.left+=1;}}
fn read(p:&Pair)->u16{p.left+p.right}
fn edit(p:&mut Pair,amount:u16){p.left+=amount;p.right=9;}
fn relay(p:&mut Pair){edit(p,4);if read(p)!=17{halt(0xba01);}}
fn number(p:&mut u16){*p+=3;*p=*p*2;}
fn identity(p:&Pair)->&Pair{p}
fn main(){
 let mut p=Pair{left:4,right:5};
 relay(&mut p);
 let shared=&p;
 if !Mode::B.same(&Mode::B){halt(0xba08);}
 if shared.sum()!=17||read(identity(shared))!=17{halt(0xba02);}
 let temporary=&Pair{left:20,right:21};if read(temporary)!=41{halt(0xba07);}
 let mut nested=Nested{pair:Pair{left:2,right:3}};
 edit(&mut nested.pair,6);
 if nested.pair.left!=8||nested.pair.right!=9{halt(0xba03);}
 let mut elements:Buf<Pair,2>=Buf::new([Pair{left:2,right:3};2]);
 edit(&mut elements[1u16],8);
 if elements[1u16].left!=10||elements[0u16].left!=2{halt(0xba04);}
 let mut n:u16=5;number(&mut n);if n!=16{halt(0xba05);}
 let borrowed=&mut p;borrowed.bump();if borrowed.left!=9{halt(0xba06);}
 halt(0x600d);
}
"#,
    );
}

#[test]
fn rustc_ordinary_references_select_addresses_without_copying_values() {
    execute(
        "ordinary-reference-selection",
        r#"
struct Pair{left:u16,right:u16}
fn read(p:&Pair)->u16{p.left+p.right}
fn choose(flag:bool,a:&Pair,b:&Pair)->u16{let p=if flag{let q=a;q}else if a.left==0{a}else{b};read(p)}
fn main(){
 let mut a=Pair{left:2,right:3};let mut b=Pair{left:7,right:11};
 if choose(true,&a,&b)!=5||choose(false,&a,&b)!=18{halt(0xbad);}
 let mut selected=&mut a;selected.left=13;selected=&mut b;selected.right=17;
 if a.left!=13||b.right!=17{halt(0xbad);}
 let answer={let p=&b;read(p)};if answer!=24{halt(0xbad);}
 halt(0x600d);
}
"#,
    );
}

#[test]
fn rustc_ordinary_references_reject_aliases_and_dangling_returns() {
    for (label, source, diagnostic) in [
        (
            "reference-alias",
            "struct P{x:u16} fn main(){let mut p=P{x:1};let a=&p;let b=&mut p;b.x=2;let x=a.x;}",
            "rustc E0502",
        ),
        (
            "reference-escape",
            "struct P{x:u16} fn bad()->&P{let p=P{x:1};&p} fn main(){}",
            "rustc E0106",
        ),
        (
            "reference-shared-write",
            "struct P{x:u16} fn bad(p:&P){p.x=2;} fn main(){}",
            "rustc E0594",
        ),
        (
            "reference-local-return",
            "struct P{x:u16} fn bad(input:&P)->&P{let p=P{x:1};&p} fn main(){}",
            "rustc E0515",
        ),
    ] {
        let error = parse(label, source)
            .err()
            .expect("invalid Rust borrow must fail")
            .to_string();
        assert!(error.contains(diagnostic), "{label}: {error}");
    }
}
