###
---

### check out report.json, it was made by doing this:
```bash
cargo run -- --root ../classified --src src --json scan
```

###
---
###

### if you wanna see the crate it was made from, go check out:

```txt
https://github.com/jameshuntt/classified
```

###
---
###
### it is designed to make file headers and documentations that explains itself
### it is intended to be a templating engine for max flexibility
###
---
###
# intended to produce things like this on command:


```txt
//! ----------------------------------------------
//! DOCUMENT DETAILS -----------------------------
//! 
//! filename:thread_pool_manager.rs
//! description:
//! usages:none in crate yet
//! 
//! ----------------------------------------------
//! FEATURE NOTES --------------------------------
//! 
//! feature_name:async
//! deps:[tokio][async_trait]
//! scope:[impl ThreadPoolManager]
//! corpus:true
//! 
//! feature_name:std
//! deps:[std]
//! scope:[impl ThreadPoolManager]
//! corpus:false
//! 
//! ----------------------------------------------
//! CORPUS FEATURES ------------------------------
//! 
#![cfg(feature = "async")]
#![cfg(feature = "std")]
```