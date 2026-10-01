mod notes;

#[aztec]
contract Toy {
    #[external("private")]
    fn transfer(x: Field) -> Field { x }

    #[external("public")]
    fn mint(x: Field) {}

    #[contract_library_method]
    fn check(x: Field) { assert(x != 0); }
}
