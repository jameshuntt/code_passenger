//! An attribute macro: it gives the item back as it is, as a stand-in for one that lays fields out its own way.

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn keep(_args: TokenStream, item: TokenStream) -> TokenStream {
    item
}
