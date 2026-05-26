# Changelog

## [0.6.0](https://github.com/lobinuxsoft/tatu/compare/v0.5.0...v0.6.0) (2026-05-26)


### Features

* **bridge:** value cheats + freeze end-to-end (SymbolDeref + Bridge freeze) ([b7815c1](https://github.com/lobinuxsoft/tatu/commit/b7815c1d1bc160aedcf55ee24d94982102ead9a1))
* **ce-launcher:** add CE Linux 7.6.6 binary management ([e25ae9a](https://github.com/lobinuxsoft/tatu/commit/e25ae9a5f5e91a66c5603051cf249421af569ca8))
* **ce-launcher:** CE Linux 7.6.6 binary management ([#59](https://github.com/lobinuxsoft/tatu/issues/59) subtask 1) ([b9195be](https://github.com/lobinuxsoft/tatu/commit/b9195bebc0ec69e1eb037bf24de6d0d58ffe834e))
* **ce-launcher:** inject Lua auto-attach into .CT tables ([654a466](https://github.com/lobinuxsoft/tatu/commit/654a466ce100e27afab327a5dea8a076b5686804))
* **ce-launcher:** Lua auto-attach injector for .CT tables ([#59](https://github.com/lobinuxsoft/tatu/issues/59) subtask 3) ([66d86df](https://github.com/lobinuxsoft/tatu/commit/66d86df1c5321b0e9d2bcfb9b3a271c447254cbb))
* cheat-core foundation + WriteOnce vertical slice ([a505687](https://github.com/lobinuxsoft/tatu/commit/a50568764db556d2b67629ca6c7bede5711034fe))
* **cheat-core:** add AddressSpec::Absolute for ad-hoc addresses ([701fe3e](https://github.com/lobinuxsoft/tatu/commit/701fe3e1a85d0a3225d38f05667a5425ac53cc80))
* **cheat-core:** add attach + memory modules ([0f6a017](https://github.com/lobinuxsoft/tatu/commit/0f6a017452374c5ec5d99a8f576466c33dfea822))
* **cheat-core:** add CT (Cheat Engine table) parser ([f2a92af](https://github.com/lobinuxsoft/tatu/commit/f2a92af0abe2de5057c8b70ec3bc5ab55d3fc9b9))
* **cheat-core:** add Freeze action variant + ActionMismatch error ([c971215](https://github.com/lobinuxsoft/tatu/commit/c97121564b0e1b5b407535a94dd20e404d0df0bf))
* **cheat-core:** add freeze module with cancellable per-cheat worker ([a4a91c1](https://github.com/lobinuxsoft/tatu/commit/a4a91c109c2f616cd0c4348df50df150dca756eb))
* **cheat-core:** add import_ct CLI + tolerate symbolic offsets ([20b916e](https://github.com/lobinuxsoft/tatu/commit/20b916ef50b32cf3bffc7303bf457839d004a503))
* **cheat-core:** add PointerChain address spec ([ff5c66a](https://github.com/lobinuxsoft/tatu/commit/ff5c66a3098e986572f6d6cc1d707b8e78f18439))
* **cheat-core:** add resolve + db modules ([b176fd6](https://github.com/lobinuxsoft/tatu/commit/b176fd6687930662294dc46bfb703a65737a448a))
* **cheat-core:** add types module with CheatTable JSON schema ([9956e18](https://github.com/lobinuxsoft/tatu/commit/9956e184161aeb153d79940cc8773a565fad5597))
* **cheat-runtime:** .CT XML → manifest auto-importer ([e091d59](https://github.com/lobinuxsoft/tatu/commit/e091d59a9adabc999cf3666139a741535b2a9811))
* **cheat-runtime:** .CT XML → manifest auto-importer ([ce72f1a](https://github.com/lobinuxsoft/tatu/commit/ce72f1a572ed9ab6ddf05ee0d678062c6ad1245f))
* **cheat-runtime:** asm bridge for jmp/call/ret via iced-x86 ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase B v1) ([9026029](https://github.com/lobinuxsoft/tatu/commit/9026029c65b9507fbf4515a2a2d212de0b0b0031))
* **cheat-runtime:** asm bridge for jmp/call/ret via iced-x86 ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase B v1) ([6e31b3e](https://github.com/lobinuxsoft/tatu/commit/6e31b3e76b04c76fea0f5d2f6f597353fc2e1de4))
* **cheat-runtime:** asm bridge v2.1 — push/pop/mov/jcc ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase B v2.1) ([cf49d9c](https://github.com/lobinuxsoft/tatu/commit/cf49d9c8fee40ebc025ae4e42a7f30d2cfc1d633))
* **cheat-runtime:** asm bridge v2.1 — push/pop/mov/jcc ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase B v2.1) ([e4800e5](https://github.com/lobinuxsoft/tatu/commit/e4800e5bb5c98650cf5e2b9416a358214f1d5a0b))
* **cheat-runtime:** Aurora JSON loader ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 5) ([b7f02de](https://github.com/lobinuxsoft/tatu/commit/b7f02debde48975d0d88091966e62905cf23ef24))
* **cheat-runtime:** Aurora JSON loader ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 5) ([fe0782e](https://github.com/lobinuxsoft/tatu/commit/fe0782e5aea7f84e169bff91755e92d8cc22f9fe))
* **cheat-runtime:** BackendKind discriminator on PersistedHook ([040ecd4](https://github.com/lobinuxsoft/tatu/commit/040ecd4f1ca10971b3a43407b3f97a8a2e0ad5fe))
* **cheat-runtime:** BridgeClient — Linux-side dial of tatu-bridge AF_UNIX ([3024dc7](https://github.com/lobinuxsoft/tatu/commit/3024dc7c948e5ef6b30c6cbdf51a7add713e0922))
* **cheat-runtime:** CE Auto-Assembler parser ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 3) ([a7879f4](https://github.com/lobinuxsoft/tatu/commit/a7879f48faf0de39ffccf5a14e2a1cea317864ff))
* **cheat-runtime:** CE Auto-Assembler parser ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 3) ([3807595](https://github.com/lobinuxsoft/tatu/commit/380759568abbb4c72bdbcd103ce40591b5fe1dfc))
* **cheat-runtime:** CE pointer-chain Value support ([91babb7](https://github.com/lobinuxsoft/tatu/commit/91babb74876c7ba4c1e0020e1ae112d81e8141af))
* **cheat-runtime:** close Phase B — anon labels + lea/add/sub/xor ([#76](https://github.com/lobinuxsoft/tatu/issues/76)) ([65f3834](https://github.com/lobinuxsoft/tatu/commit/65f38346dbaf3e56976ee1fa9c049e96b17742c0))
* **cheat-runtime:** close Phase B — anon labels + lea/add/sub/xor ([#76](https://github.com/lobinuxsoft/tatu/issues/76)) ([fa0b5bd](https://github.com/lobinuxsoft/tatu/commit/fa0b5bde079ac7007ac3ac09494a18537833020c))
* **cheat-runtime:** close Phase E — extension cdylib + ptrace dlopen ([#76](https://github.com/lobinuxsoft/tatu/issues/76) final) ([747c3b5](https://github.com/lobinuxsoft/tatu/commit/747c3b58c5e9bb20eac0898193304f50791561cf))
* **cheat-runtime:** close Phase E — extension cdylib + ptrace dlopen ([#76](https://github.com/lobinuxsoft/tatu/issues/76)) ([060f0a9](https://github.com/lobinuxsoft/tatu/commit/060f0a9a1a2e5009946e7da4abbd7ed82273cc17))
* **cheat-runtime:** compose alloc + asm into full code injection ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase C) ([74e8b0e](https://github.com/lobinuxsoft/tatu/commit/74e8b0e8f6268563102f84a59aebf6bb03396551))
* **cheat-runtime:** compose alloc + asm into full code injection ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase C) ([febf5e8](https://github.com/lobinuxsoft/tatu/commit/febf5e8ea07dd1b91ba051ebfbee0cf0d38c1905))
* **cheat-runtime:** deprecate cheat-core ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 7-A) ([53e91ea](https://github.com/lobinuxsoft/tatu/commit/53e91ea0c6eb44a18d14ec050943f0464c437ac2))
* **cheat-runtime:** deprecate cheat-core ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 7-A) ([25c67bc](https://github.com/lobinuxsoft/tatu/commit/25c67bc4b3e5cd4a1485178d0a26d73e3a210ddc))
* **cheat-runtime:** elfsym module + symbol/module lookup ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase D) ([a02d02a](https://github.com/lobinuxsoft/tatu/commit/a02d02a8e3ba7715182093342856c807686542ab))
* **cheat-runtime:** elfsym module + symbol/module lookup ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase D) ([b0334b8](https://github.com/lobinuxsoft/tatu/commit/b0334b89e86427c0dd64ea77ba9cabdf311c8ef4))
* **cheat-runtime:** Engine routes I/O through MemoryAccess ([668aabe](https://github.com/lobinuxsoft/tatu/commit/668aabed0c770317e1e373a2e8d7eb34261fac8a))
* **cheat-runtime:** executor with atomic enable/disable ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 4) ([aaba09b](https://github.com/lobinuxsoft/tatu/commit/aaba09baae2612bb3a107dd152530d03e64debf3))
* **cheat-runtime:** executor with atomic enable/disable ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 4) ([0b79fbf](https://github.com/lobinuxsoft/tatu/commit/0b79fbf3ecb13bf944003ac7cba7e48f940d584f))
* **cheat-runtime:** GetThreadContext + SetThreadContext + DR0-DR7 helpers (closes [#140](https://github.com/lobinuxsoft/tatu/issues/140)) ([9e79d43](https://github.com/lobinuxsoft/tatu/commit/9e79d43d5aa5aab97c7f8e061eb86062bc29931d))
* **cheat-runtime:** GetThreadContext + SetThreadContext + DR0-DR7 helpers (closes [#140](https://github.com/lobinuxsoft/tatu/issues/140)) ([795939b](https://github.com/lobinuxsoft/tatu/commit/795939bbc10f3bfc2ad153b4671ffe9c3f34aac0))
* **cheat-runtime:** manifest Value kind + ct_import emits Value features ([9de83a5](https://github.com/lobinuxsoft/tatu/commit/9de83a52a82010a3e606b7c40fbb51921a67f9f3))
* **cheat-runtime:** ManifestFeature kind (Toggle / Header) ([#80](https://github.com/lobinuxsoft/tatu/issues/80)) ([d8b52a8](https://github.com/lobinuxsoft/tatu/commit/d8b52a8a7e88d3e6d59a4779845a11d6528e2482))
* **cheat-runtime:** ManifestFeature kind (Toggle / Header) ([#80](https://github.com/lobinuxsoft/tatu/issues/80)) ([bff25c5](https://github.com/lobinuxsoft/tatu/commit/bff25c52130892c13a2f69e6b90309a62666a721))
* **cheat-runtime:** mask-aware AOB scanner ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 2) ([1698ea0](https://github.com/lobinuxsoft/tatu/commit/1698ea026d64e2ac4b5f8e782a85c2e08f231132))
* **cheat-runtime:** mask-aware AOB scanner ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 2) ([11e6bbe](https://github.com/lobinuxsoft/tatu/commit/11e6bbef424e2d4f6d83d47e5b4fffeed3740e7f))
* **cheat-runtime:** memory operands + float literals ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase B v2.2) ([eabaac0](https://github.com/lobinuxsoft/tatu/commit/eabaac02978c5bc130110d76ec188fd6786eb574))
* **cheat-runtime:** memory operands + float literals ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase B v2.2) ([dd90056](https://github.com/lobinuxsoft/tatu/commit/dd900563e2e201579eac0f13ba51d9add95f9c12))
* **cheat-runtime:** persist undo log + orphan-hook recovery ([1b80e86](https://github.com/lobinuxsoft/tatu/commit/1b80e8642c87651b8c8a2c6cab3d5bf7fd7ce255))
* **cheat-runtime:** persist undo log + orphan-hook recovery (closes [#97](https://github.com/lobinuxsoft/tatu/issues/97)) ([eca60c1](https://github.com/lobinuxsoft/tatu/commit/eca60c12c01e35fbdbe4781dfe63721ec071dfb2))
* **cheat-runtime:** Phase 1 — workspace bootstrap + DLL skeleton ([#102](https://github.com/lobinuxsoft/tatu/issues/102)) ([8316542](https://github.com/lobinuxsoft/tatu/commit/83165423274fa79f222eb7c9c28ba539986dac24))
* **cheat-runtime:** Phase 2 — dinput8.dll proxy forwarders ([#102](https://github.com/lobinuxsoft/tatu/issues/102)) ([9a85961](https://github.com/lobinuxsoft/tatu/commit/9a8596134241e37976e668195bba1433413b83a0))
* **cheat-runtime:** pointer-chain walker + address expression parser ([be0fe33](https://github.com/lobinuxsoft/tatu/commit/be0fe33e1d43e36074f83c8ab93be8433e6437f9))
* **cheat-runtime:** port debug subsystem (StartDebug/StopDebug/SetBp/RemoveBp/WaitEvent/Continue) (closes [#142](https://github.com/lobinuxsoft/tatu/issues/142), supersedes [#132](https://github.com/lobinuxsoft/tatu/issues/132)) ([cabecb9](https://github.com/lobinuxsoft/tatu/commit/cabecb9ffd6994bf7353117fa8aced1bcec1be07))
* **cheat-runtime:** port debug subsystem completo (StartDebug/StopDebug/SetBp/RemoveBp/WaitEvent/Continue) (closes [#142](https://github.com/lobinuxsoft/tatu/issues/142), supersedes [#132](https://github.com/lobinuxsoft/tatu/issues/132)) ([9f23533](https://github.com/lobinuxsoft/tatu/commit/9f235339e782b601f11d1dbdd9301a04bbc4e12c))
* **cheat-runtime:** PTRACE_PEEKDATA/POKEDATA memory fallback + Debugger::read_memory_debug/write_memory_debug (closes [#143](https://github.com/lobinuxsoft/tatu/issues/143)) ([9c8a8e7](https://github.com/lobinuxsoft/tatu/commit/9c8a8e7c441bdee33ac5d9646fa3538517c113e3))
* **cheat-runtime:** PTRACE_PEEKDATA/POKEDATA memory fallback + Debugger::read_memory_debug/write_memory_debug (closes [#143](https://github.com/lobinuxsoft/tatu/issues/143)) ([323e8af](https://github.com/lobinuxsoft/tatu/commit/323e8af68dab7fd028c51426257d945a4e442e67))
* **cheat-runtime:** remote alloc/dealloc via ptrace ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase A) ([037078b](https://github.com/lobinuxsoft/tatu/commit/037078b945351da75940690b5a0deede518f17b0))
* **cheat-runtime:** remote alloc/dealloc via ptrace ([#76](https://github.com/lobinuxsoft/tatu/issues/76) Phase A) ([6048927](https://github.com/lobinuxsoft/tatu/commit/60489271d6e691adaf962cb84246243563dc1c10))
* **cheat-runtime:** remove cheat-core + migrate legacy JSONs ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 7-B) ([de1b41d](https://github.com/lobinuxsoft/tatu/commit/de1b41db16a0615ef5c38c649335741c2e68ad1f))
* **cheat-runtime:** remove cheat-core + migrate legacy JSONs ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 7-B) ([eddbe46](https://github.com/lobinuxsoft/tatu/commit/eddbe46d283b65abd9752d64c5279435e286322e))
* **cheat-runtime:** safe_ptrace wrapper + attach_and_wait + SIGCHLD handler (closes [#139](https://github.com/lobinuxsoft/tatu/issues/139)) ([f98430e](https://github.com/lobinuxsoft/tatu/commit/f98430e0d19c46a9be940be4fe0e67a572a48193))
* **cheat-runtime:** safe_ptrace wrapper + attach_and_wait + SIGCHLD handler (closes [#139](https://github.com/lobinuxsoft/tatu/issues/139)) ([0212421](https://github.com/lobinuxsoft/tatu/commit/02124215e15325564a630ff76bac0453c04b1cec))
* **cheat-runtime:** scaffold + memory R/W layer ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 1) ([776ebc4](https://github.com/lobinuxsoft/tatu/commit/776ebc412b84ce9e7f1a9976d6233b87020dc6fd))
* **cheat-runtime:** scaffold crate + memory R/W via process_vm_readv ([973d45c](https://github.com/lobinuxsoft/tatu/commit/973d45c26b5e1e2045fc18a25f86afd3a3b08fbb))
* **cheat-runtime:** seal API for downstream reuse ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 8) ([9ce9df6](https://github.com/lobinuxsoft/tatu/commit/9ce9df6eb4143fce5719852cf8dfd3a8efa8db4a))
* **cheat-runtime:** seal API for downstream reuse ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 8) ([292b27d](https://github.com/lobinuxsoft/tatu/commit/292b27d3e528ea38f8880e4513c775b74209366c))
* **cheat-runtime:** SuspendThread/ResumeThread/FindPausedThread con ref-count por TID (closes [#141](https://github.com/lobinuxsoft/tatu/issues/141)) ([6e9bff2](https://github.com/lobinuxsoft/tatu/commit/6e9bff2c95a58f1ff00d6591b88d0beb694b4c83))
* **cheat-runtime:** SuspendThread/ResumeThread/FindPausedThread con ref-count por TID (closes [#141](https://github.com/lobinuxsoft/tatu/issues/141)) ([e11db93](https://github.com/lobinuxsoft/tatu/commit/e11db93ab3047fee1af52591b0b8a2d567dfea61))
* **cheat-runtime:** SymbolTable multi-module + enumerate_modules + spec grammar (closes [#144](https://github.com/lobinuxsoft/tatu/issues/144)) ([33f3d5f](https://github.com/lobinuxsoft/tatu/commit/33f3d5f47ee88feb37a34a25b320d2306292a94c))
* **cheat-runtime:** SymbolTable multi-module + enumerate_modules + spec grammar (closes [#144](https://github.com/lobinuxsoft/tatu/issues/144)) ([442c464](https://github.com/lobinuxsoft/tatu/commit/442c4646778f0bc60142d2eb9bcf8addbffccd24))
* **cheat-runtime:** Tauri integration + manifest format + PID finder ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 6) ([c2b6cda](https://github.com/lobinuxsoft/tatu/commit/c2b6cda43e4c2560491884c82fe38dd58a9cd581))
* **cheat-runtime:** Tauri integration + manifest format + PID finder ([#64](https://github.com/lobinuxsoft/tatu/issues/64) subtask 6) ([4afcfb1](https://github.com/lobinuxsoft/tatu/commit/4afcfb1945530719a41c6af40e6796fdc8bd3da0))
* **cheat-runtime:** VirtualQueryEx + region enumeration + page protection (closes [#138](https://github.com/lobinuxsoft/tatu/issues/138)) ([0508870](https://github.com/lobinuxsoft/tatu/commit/0508870fb5447ed15d13b3528df47a74fd1542b2))
* **cheat-runtime:** VirtualQueryEx + region enumeration cache (closes [#138](https://github.com/lobinuxsoft/tatu/issues/138)) ([ffe800c](https://github.com/lobinuxsoft/tatu/commit/ffe800c9db8c70b200f5b41e8029439fc5bdd814))
* **cheat:** add Cheats tab to detail panel ([74e5130](https://github.com/lobinuxsoft/tatu/commit/74e513087ea88bfd9d36c38ae5aac5370117e419))
* **cheat:** Cheat Engine .CT importer + parse hardening ([aa1bdd3](https://github.com/lobinuxsoft/tatu/commit/aa1bdd3bf057e59b8469656e9ee36d4662288b61))
* **cheat:** expose Freeze toggle/status commands + action_kind in summary ([62eec93](https://github.com/lobinuxsoft/tatu/commit/62eec93119d9f42487477748e98ce61e0faf7ffb))
* **cheat:** Freeze action with toggle UI ([e753acb](https://github.com/lobinuxsoft/tatu/commit/e753acb05d8bba0e3f3a437a1dd9b756deb204f8))
* **cheats:** add Fearless Revolution search button per game ([e3c9b43](https://github.com/lobinuxsoft/tatu/commit/e3c9b4360ccc9a02dad6b880652cd5d0cff94dbe))
* **cheats:** Fearless Revolution search button ([#59](https://github.com/lobinuxsoft/tatu/issues/59) subtask 2, path D) ([22c51d1](https://github.com/lobinuxsoft/tatu/commit/22c51d193b29c00b3dc045d31eb9f12004e3a96a))
* **cheats:** Open CE button + .CT listing + game exe detection ([553f3f2](https://github.com/lobinuxsoft/tatu/commit/553f3f255179fb7a9fb08ce1ed538735a672e886))
* **cheats:** Open CE button + .CT listing + smoke ([#59](https://github.com/lobinuxsoft/tatu/issues/59) final) ([c010dc4](https://github.com/lobinuxsoft/tatu/commit/c010dc4a1ad35f41d08d406851f9f9b8b20cdadc))
* **cheat:** toggle switch UI for Freeze action ([cb9c597](https://github.com/lobinuxsoft/tatu/commit/cb9c597939f50499287139c030549d3f46df8008))
* **cheat:** wire cheat-core into Tauri backend ([e377e5e](https://github.com/lobinuxsoft/tatu/commit/e377e5eac6c8651e4df2514949a9ad616146cbcd))
* **dll:** add cheat-runtime-dll crate with DllMain skeleton ([22ab19e](https://github.com/lobinuxsoft/tatu/commit/22ab19e12f047a2ef4979dc08ba277ec600992b9))
* **dll:** forward six dinput8.dll exports through to system32 real ([440e44a](https://github.com/lobinuxsoft/tatu/commit/440e44ad2e142deb6d2603c49e51f38c9096dbc6))
* **frontend:** Proton picker dropdown + launcher.toml writes on toggle ([a4e5246](https://github.com/lobinuxsoft/tatu/commit/a4e524659f969e7862beb829d2637cbc35bd20f5))
* **frontend:** Tatu Launcher backend banner with per-game toggle ([edf6644](https://github.com/lobinuxsoft/tatu/commit/edf6644916c2b3f962626da3b24d958f8bfa80a0))
* Phase 5 — MemoryAccess trait + dedup scanner/chain across backends ([8b17f13](https://github.com/lobinuxsoft/tatu/commit/8b17f13a4143776e8531142a627fcd902fabb2ee))
* Phase 6 — per-game backend routing (value cheats + recovery) ([6d28449](https://github.com/lobinuxsoft/tatu/commit/6d28449cf2191b7a6f0eff0286975e54f9d37dbe))
* Phase 7B — Win32Backend + EnableScript wire (closes Phase 6 deferred) ([1188884](https://github.com/lobinuxsoft/tatu/commit/11888844f607d6ad9350b2bae70379b23d7aaa14))
* Phase 7B — Win32Backend + EnableScript wire (closes Phase 6 deferred) ([edcd081](https://github.com/lobinuxsoft/tatu/commit/edcd08137db4bdae6929dfb89e8b9d3897839c29))
* Phase 8 wiring — Aurora-style TCP bridge + cross-process write safety (smoke validated) ([f56c4ae](https://github.com/lobinuxsoft/tatu/commit/f56c4aebe92e61e8e2f253ce7e1e0b078f3a9425))
* **proto:** add cheat-runtime-proto wire-type crate ([c425893](https://github.com/lobinuxsoft/tatu/commit/c425893ae627427083d09ea3309aefcc8a3ded6d))
* **tatu-bridge,cheat-runtime:** Phase 3 — IPC transport tracker ↔ bridge (epic [#106](https://github.com/lobinuxsoft/tatu/issues/106)) ([e56e71d](https://github.com/lobinuxsoft/tatu/commit/e56e71d4a8ce91c69af7fca5bd87069be8d430f6))
* **tatu-bridge:** AF_UNIX server in --connect mode (CHRT v1) ([4aa045a](https://github.com/lobinuxsoft/tatu/commit/4aa045a85a59f5a96ec799e3f190ad00d92753e8))
* **tatu-bridge:** AOB scanner over remote process via Win32 ([159a347](https://github.com/lobinuxsoft/tatu/commit/159a34783d5e1bb33c5cdc9e005c55c27df5fc47))
* **tatu-bridge:** code patcher with SuspendThread RAII ([a4510aa](https://github.com/lobinuxsoft/tatu/commit/a4510aa8b2edcf6ee6b22e0bb981e5b73bb927a0))
* **tatu-bridge:** codecave allocator via VirtualAllocEx + free ([1b7acd6](https://github.com/lobinuxsoft/tatu/commit/1b7acd654c0d001c05456f3f6223525e7bfa0384))
* **tatu-bridge:** Phase 4 — in-process primitives (AOB / patch / alloc / chain) ([ec187d9](https://github.com/lobinuxsoft/tatu/commit/ec187d9c86d736ec631553929426ca38f6453e5d))
* **tatu-bridge:** pointer-chain walker + typed read/write ([6a74e01](https://github.com/lobinuxsoft/tatu/commit/6a74e01f527e4291d6ab6a64dc69a8d83c74392e))
* **tatu-engine:** Backend trait + Engine&lt;B&gt; generic (Phase 7A2) ([703cf27](https://github.com/lobinuxsoft/tatu/commit/703cf27c9d251b42c773d5a72593c05b8b045ccb))
* **tatu-engine:** Backend trait + Engine&lt;B&gt; generic over backends ([6c6ecfc](https://github.com/lobinuxsoft/tatu/commit/6c6ecfc5f8a577a386989a105f9a3f39a6b23f97))
* **tatu-engine:** complete AA command coverage for common .CT tables ([1e381e6](https://github.com/lobinuxsoft/tatu/commit/1e381e688504f268f662386131e74da40b090066))
* **tatu-engine:** new crate — move parser + asm out of cheat-runtime ([56ca88b](https://github.com/lobinuxsoft/tatu/commit/56ca88b5e0299303993c3752f739b459f2b2316b))
* **tatu-engine:** new crate — move parser + asm out of cheat-runtime (Phase 7A1) ([cb30942](https://github.com/lobinuxsoft/tatu/commit/cb30942e0d27e3ce8b16c9c2396590a8f5b073a6))
* **tatu-engine:** wildcard + multi-arg name lists in (un)registersymbol/label/dealloc ([6f80854](https://github.com/lobinuxsoft/tatu/commit/6f808548f0d162d199e64115e61be9e8518301d2))
* **tatu-launcher:** install.sh drop-in + build script + CI job ([a87d8bb](https://github.com/lobinuxsoft/tatu/commit/a87d8bbfc399e86c68708fb2502a046040a6fee0))
* **tatu-launcher:** Phase 2 — Steam compat tool packaging (epic [#106](https://github.com/lobinuxsoft/tatu/issues/106)) ([bc15f80](https://github.com/lobinuxsoft/tatu/commit/bc15f8077b69e1b9723bc3763a30312ba616dc3b))
* **tatu-launcher:** Rust binary — verb routing + Proton resolver + TOML config ([a39c23e](https://github.com/lobinuxsoft/tatu/commit/a39c23e1ec49eaee277369d2940373e7dba590fa))
* **tatu-launcher:** Steam compat tool skeleton (toolmanifest.vdf + wrapper) ([23bdbea](https://github.com/lobinuxsoft/tatu/commit/23bdbeadd9adc43e63a7d3b15ab0204bd8c1dd27))
* **tatu-launcher:** writer + upsert/remove on Config ([cfdb2b3](https://github.com/lobinuxsoft/tatu/commit/cfdb2b3c124971c9064a4a200e490847a39cc184))
* **tatu-mem:** new crate — MemoryAccess trait + shared pure-logic ([4991693](https://github.com/lobinuxsoft/tatu/commit/4991693b0e827bf511fd3b93e3da2dda700011a1))
* **tatu-proto:** extend CHRT v1 with Phase 4 primitive requests ([346c05c](https://github.com/lobinuxsoft/tatu/commit/346c05c7ffa3fac3e53f66f2f739be34bc778202))
* **tatu:** pivot to Aurora-style Win32 bridge (rename + restructure, [#106](https://github.com/lobinuxsoft/tatu/issues/106) Phase 1) ([b84da3d](https://github.com/lobinuxsoft/tatu/commit/b84da3d01d647ccc3bd699ddd88cd40c8536c0c8))
* **tauri:** value_read / value_write / value_freeze commands ([5914fcf](https://github.com/lobinuxsoft/tatu/commit/5914fcfabf7dcc6c7a6a1ba56bf87f4b188aae97))
* **tracker:** Bridge liveness check in purge_stale_cheats ([96a8cbf](https://github.com/lobinuxsoft/tatu/commit/96a8cbf5eb9f4b075e1d76d7d8776d23d55f9190))
* **tracker:** cheat_runtime_backend_recommend + tatu_launcher commands ([f683497](https://github.com/lobinuxsoft/tatu/commit/f683497cf6fe6f6accfd2a03d6cafb4305a1dc9f))
* **tracker:** per-game backend selection + Tauri commands ([9c079bb](https://github.com/lobinuxsoft/tatu/commit/9c079bba07e67602fc29fd931fb8b8579d2f4d12))
* **tracker:** Phase 7C — Tatu Launcher auto-install + per-game backend toggle ([a22f643](https://github.com/lobinuxsoft/tatu/commit/a22f6433fb3aeaed834fa69b26fabc7e712bcfed))
* **tracker:** Proton enumerator + launcher_config Tauri commands ([d7fad04](https://github.com/lobinuxsoft/tatu/commit/d7fad046e51b724ec5787af91e0387f8c6b29d13))
* **tracker:** route orphan-hook recovery through the backend ([e091aa5](https://github.com/lobinuxsoft/tatu/commit/e091aa53ee71bcc3ebfe321cf2516e5641d0f3c2))
* **tracker:** route value_read/write through the bridge ([1655882](https://github.com/lobinuxsoft/tatu/commit/1655882379a73ab744ba483e994b1209f0a8b3df))
* **tracker:** Steam wineprefix walker + library_paths consolidation ([eb2f9c8](https://github.com/lobinuxsoft/tatu/commit/eb2f9c8ccdb5bad49f86d2d282f98da06dbb9298))
* **tracker:** tatu_launcher module (status / install / config.vdf patch) ([e0efc56](https://github.com/lobinuxsoft/tatu/commit/e0efc56116a5432efd41368a47f97000791ab44f))
* **ui:** Value control row — read / set / freeze + master-gate ([31f33fd](https://github.com/lobinuxsoft/tatu/commit/31f33fdee84d9bb4681b224d2f8554e8908e712f))


### Bug Fixes

* **alloc:** place codecave within ±2GB of near hint via MAP_FIXED_NOREPLACE ([d561788](https://github.com/lobinuxsoft/tatu/commit/d56178813e2088ffbe48e0fa639f6507fa2dd25c))
* **appimage:** quote AppImage path in .desktop Exec line ([85741ca](https://github.com/lobinuxsoft/tatu/commit/85741ca033f275054b8009a9716c75611d3bdf32))
* **appimage:** SIGSEGV at dl_init on Bazzite F44 (closes [#65](https://github.com/lobinuxsoft/tatu/issues/65)) ([3355d9f](https://github.com/lobinuxsoft/tatu/commit/3355d9f23e2b4ae3551e3355a2c4fdebafaa7dc7))
* **appimage:** SIGSEGV at dl_init on Bazzite F44 (closes [#65](https://github.com/lobinuxsoft/tatu/issues/65)) ([e2fd146](https://github.com/lobinuxsoft/tatu/commit/e2fd146a8d5d9614068b75f287c681cb01551450))
* **asm,alloc:** make CE-style trampolines compile under x86_64 Linux ([6a6a499](https://github.com/lobinuxsoft/tatu/commit/6a6a499c3ec3821739c5a9bf39eb2901f7f2276d))
* **asm:** parse bare [reg+N] / [N] displacements as hex (CE-AA convention) ([ef7ad65](https://github.com/lobinuxsoft/tatu/commit/ef7ad653728bd2cf02d09af2f6a47d059e9e9c8b))
* **cheat-core:** match exe_pattern only against cmdline[0] ([138e5e3](https://github.com/lobinuxsoft/tatu/commit/138e5e3afd7d4171c1faa2661d8032f17058564f))
* **cheat-runtime:** atomic rollback + surface errors ([83a99bc](https://github.com/lobinuxsoft/tatu/commit/83a99bcd3e8b732ce031eaa8326926e29169059b))
* **cheat-runtime:** atomic rollback + surface errors (closes [#95](https://github.com/lobinuxsoft/tatu/issues/95)) ([e601ab1](https://github.com/lobinuxsoft/tatu/commit/e601ab18aafb2f064bb7731e699e59a564fff574))
* **cheat-runtime:** orphans_list excludes live in-memory cheats ([ea14e7e](https://github.com/lobinuxsoft/tatu/commit/ea14e7ea7e92a79c700af04437aa275ee0e58891))
* **cheat-runtime:** orphans_list excludes live in-memory cheats (closes [#100](https://github.com/lobinuxsoft/tatu/issues/100)) ([f54d37f](https://github.com/lobinuxsoft/tatu/commit/f54d37f2c1e49ae4b9057a51a605f1497695f6f3))
* **cheat-runtime:** purge stale ActiveCheats on game relaunch ([d627b9f](https://github.com/lobinuxsoft/tatu/commit/d627b9fa16dc95d1abd34c9b668eb8c94bed99c6))
* **cheat-runtime:** read ELF magic from offset==0 region ([461a793](https://github.com/lobinuxsoft/tatu/commit/461a7937fbb0165f93a6de451aa45538cc8c1bb2))
* **ci:** three preexisting CI fails (clippy + ELF test + script exec bit) ([f938754](https://github.com/lobinuxsoft/tatu/commit/f938754bb19a0306abb38886634a6549b862cdf8))
* **clippy:** satisfy Rust 1.95 lints ([549045d](https://github.com/lobinuxsoft/tatu/commit/549045d214c65ab70c93b601e0679eb89d0ef16a))
* **clippy:** third unnecessary_sort_by call site in manifest ([40e2e8f](https://github.com/lobinuxsoft/tatu/commit/40e2e8f6037ba65e6622851d409deb4717296fb9))
* **ct-import:** drop ornamental GroupHeader entries that drown out toggles ([ac941cd](https://github.com/lobinuxsoft/tatu/commit/ac941cda7dcee034618272d7745c11b9113633f3))
* **docs:** avoid markdown bullet ambiguity in cheat-runtime header ([ae81d2f](https://github.com/lobinuxsoft/tatu/commit/ae81d2ff64d8f96e5d9e5b427dbde10b033c222d))
* **executor:** length estimator must use cursor as base, not 0 ([2383faf](https://github.com/lobinuxsoft/tatu/commit/2383fafb2493008690d4b155dfc62a9994aaf054))
* **executor:** pause every game thread during write pass ([9838108](https://github.com/lobinuxsoft/tatu/commit/9838108d4a62b7b0f5a6f815b8ba681f28ee9268))
* **executor:** single-attach + POKEDATA writes for the whole pass ([282c099](https://github.com/lobinuxsoft/tatu/commit/282c099c967d3a6828a17a8a2243a0520f17297d))
* **steam-exe:** walk depth 5 to find UE shipping .exe ([295da2e](https://github.com/lobinuxsoft/tatu/commit/295da2e7853e2484fd14a49740c49745d483d679))
* **tatu-bridge:** batch thread suspend per Engine cycle (drops per-write churn) ([91ab3b6](https://github.com/lobinuxsoft/tatu/commit/91ab3b6b072187d156c0bada1f4c297206d897fa))
* **tatu-bridge:** batch thread suspend per Engine cycle (Phase 9.1 hardening) ([eadf06a](https://github.com/lobinuxsoft/tatu/commit/eadf06a8d29e372312adbedfd2397800b18182e9))
* **tatu-bridge:** VirtualAllocEx near-scan keeps codecaves in rel32 reach ([b5f923a](https://github.com/lobinuxsoft/tatu/commit/b5f923ae42bdc1d4ba68e67e0c1340756c41643c))
* **tatu-bridge:** Win32Backend::write goes through patch_bytes ([7bb6821](https://github.com/lobinuxsoft/tatu/commit/7bb6821eadb36ebc1d19411c0ba9673877e39ecf))
* **tauri:** purge stale ActiveCheats when the game PID is gone ([55b1c34](https://github.com/lobinuxsoft/tatu/commit/55b1c34c94915b0f27cc0c84e0cfed93b198292f))
* **tracker:** launcher.toml writer + Proton picker UI (Phase 7C bug) ([8546f52](https://github.com/lobinuxsoft/tatu/commit/8546f52d2dd6ff4c0b3cf87f9390318d9249e29a))
* **tracker:** mark bridge_chain doc pseudo-code blocks as text ([a155a67](https://github.com/lobinuxsoft/tatu/commit/a155a67e9c48ab93087f095c5b9a5ed581ac217c))


### Refactoring

* **bridge:** migrate IPC from AF_UNIX to TCP loopback ([6f3ec68](https://github.com/lobinuxsoft/tatu/commit/6f3ec68a88ae59fb6e0024e364d1aa8189d7efb2))
* **cheat-runtime:** adopt tatu-mem for scanner + chain logic ([a2a7d4b](https://github.com/lobinuxsoft/tatu/commit/a2a7d4bbaa693cbb8725a2b7b324865ffa8dee64))
* **cheat-runtime:** extract chain/ and migrate/ tests into submodules ([94172ed](https://github.com/lobinuxsoft/tatu/commit/94172ed6f7d902a660862ef3de021265015fac15)), closes [#85](https://github.com/lobinuxsoft/tatu/issues/85)
* **cheat-runtime:** split ct_import into mod/xml_walker/heuristics/disk/tests ([73f01dc](https://github.com/lobinuxsoft/tatu/commit/73f01dc20f29b4bd7798a5425413fbe967999793)), closes [#85](https://github.com/lobinuxsoft/tatu/issues/85)
* **cheat-runtime:** split executor module into engine/active/rollback/error ([f700125](https://github.com/lobinuxsoft/tatu/commit/f700125ee1f58c725efff721d209a008645c1674)), closes [#85](https://github.com/lobinuxsoft/tatu/issues/85)
* **cheat-runtime:** split monolithic asm/executor/parser into submodules ([#85](https://github.com/lobinuxsoft/tatu/issues/85)) ([d56effd](https://github.com/lobinuxsoft/tatu/commit/d56effd05286b3e166b09026518c7e7e22e30030))
* **cheat-runtime:** split monolithic asm/executor/parser into submodules ([#85](https://github.com/lobinuxsoft/tatu/issues/85)) ([5dfe2ce](https://github.com/lobinuxsoft/tatu/commit/5dfe2ce6c2909d48206940d45cffde4af6d6a743))
* **cheat-runtime:** split monolithic modules + clippy 1.93 cleanup ([cc4893c](https://github.com/lobinuxsoft/tatu/commit/cc4893cff74e9c417b48de5d7899bc9c6f86b292))
* **commands:** split cheat_runtime_cmd into features/toggles/orphans/values ([7cc04c9](https://github.com/lobinuxsoft/tatu/commit/7cc04c989a224cfc11a080b5a4679e9e9596ecd1)), closes [#85](https://github.com/lobinuxsoft/tatu/issues/85)
* **tatu-bridge:** adopt tatu-mem, drop duplicated aob + chain ([4e5992d](https://github.com/lobinuxsoft/tatu/commit/4e5992d952b4e28e41190bf2104dfb9bf72f6a02))
* **tatu-proto:** host the IPC wire protocol (move from cheat-runtime-extension) ([db54737](https://github.com/lobinuxsoft/tatu/commit/db54737be50b97684474fa2f91702a8eb99fd463))
* **workspace:** rename cheat-runtime-proto → tatu-proto, drop cheat-runtime-dll ([bb2b03b](https://github.com/lobinuxsoft/tatu/commit/bb2b03b101996aadd506bb0fc22d65291750ae03))


### Documentation

* **tatu-engine:** AA command coverage matrix (closes [#131](https://github.com/lobinuxsoft/tatu/issues/131) acceptance) ([e07598c](https://github.com/lobinuxsoft/tatu/commit/e07598c96d4c35f3300a6d854dd4bd27d19a0165))
* **tatu:** backend selection table + fallback-only markers ([22eb931](https://github.com/lobinuxsoft/tatu/commit/22eb931a009124fb8dfba1927aeee6ef8c06e78e))
* **tatu:** Phase 9 cleanup — backend selection table + fallback-only markers ([d8f2e72](https://github.com/lobinuxsoft/tatu/commit/d8f2e72cf8ee82e3782f0828608fe551172f0874))


### Tests

* **cheat-runtime:** empirical smoke validation of alloc_remote against live wine game ([0eab172](https://github.com/lobinuxsoft/tatu/commit/0eab172f3e113bdce356e5c434954ce3cb38d77c))
* **cheat-runtime:** empirical smoke validation of alloc_remote against live wine game (closes [#130](https://github.com/lobinuxsoft/tatu/issues/130)) ([f7e8df2](https://github.com/lobinuxsoft/tatu/commit/f7e8df25cdfb547018546400505b282ffb49cf8e))

## [0.5.0](https://github.com/lobinuxsoft/game-progress-tracker/compare/v0.4.0...v0.5.0) (2026-04-16)


### Features

* classify game preservability (Goldberg / GOG / DRM removal) ([4eae205](https://github.com/lobinuxsoft/game-progress-tracker/commit/4eae205483ea371b6384351a1a9a97a486275ae7))
* classify game preservability (Goldberg / GOG / DRM removal) ([8527c17](https://github.com/lobinuxsoft/game-progress-tracker/commit/8527c1751c91c2b98e1cc4bf0a4091e2cb1bef33)), closes [#28](https://github.com/lobinuxsoft/game-progress-tracker/issues/28)
* **css:** apply Cyberpunk palette + theme switcher (4 themes) ([afcc978](https://github.com/lobinuxsoft/game-progress-tracker/commit/afcc978f8a0e81d7314cda5854b1af29fce6689d))
* **css:** apply Cyberpunk palette + theme switcher (4 themes) ([471e0f6](https://github.com/lobinuxsoft/game-progress-tracker/commit/471e0f61e2de3b89b113c2f3ca952e9227266a01)), closes [#46](https://github.com/lobinuxsoft/game-progress-tracker/issues/46)
* detect DRM status for Steam games ([8ea27bb](https://github.com/lobinuxsoft/game-progress-tracker/commit/8ea27bbffa393b5fd8a838c7013e302b5bb6a56a))
* detect DRM status for Steam games ([6e34698](https://github.com/lobinuxsoft/game-progress-tracker/commit/6e3469840cdff7f127b35950e44bf6713b94fea1)), closes [#25](https://github.com/lobinuxsoft/game-progress-tracker/issues/25)
* refine DRM classification and add Steam collection import ([a880dcd](https://github.com/lobinuxsoft/game-progress-tracker/commit/a880dcd23d69e7947b2820905f72615d90dd1e75))
* rotate state.json backups with atomic writes ([27ee2b8](https://github.com/lobinuxsoft/game-progress-tracker/commit/27ee2b815dbd6217cb44541b2c76ef025063fd30))
* rotate state.json backups with atomic writes ([3c4cd41](https://github.com/lobinuxsoft/game-progress-tracker/commit/3c4cd4192e78d0792f01dedc39222e060d9af273)), closes [#30](https://github.com/lobinuxsoft/game-progress-tracker/issues/30)
* track game disk size (libraryfolders.vdf + appinfo.vdf) ([b87fa16](https://github.com/lobinuxsoft/game-progress-tracker/commit/b87fa16e89fa434b6636b63b343eddcb23aff1b0))
* track game disk size (libraryfolders.vdf + appinfo.vdf) ([5b266e6](https://github.com/lobinuxsoft/game-progress-tracker/commit/5b266e6dcab032f7d6288b3f0f14646c26facf1d)), closes [#26](https://github.com/lobinuxsoft/game-progress-tracker/issues/26)


### Bug Fixes

* use sort_by_key to satisfy clippy::unnecessary_sort_by ([88df1d2](https://github.com/lobinuxsoft/game-progress-tracker/commit/88df1d29d4401730b8167d80ca9c4057ea04d7a5))


### Refactoring

* **css:** introduce design tokens + add Cyberpunk redesign previews ([bc00138](https://github.com/lobinuxsoft/game-progress-tracker/commit/bc00138119b98bd31c0de5a920dfa91ba45000e7))
* **css:** introduce design tokens for visual redesign ([90d209e](https://github.com/lobinuxsoft/game-progress-tracker/commit/90d209ee1ca5ce651f6eef5f07a78bbe5babf1c8)), closes [#44](https://github.com/lobinuxsoft/game-progress-tracker/issues/44)
* organize styles.css (215 lines) into section files ([9462972](https://github.com/lobinuxsoft/game-progress-tracker/commit/946297262320a380bc6cf2d4109191ce8a14c888))
* organize styles.css into section files ([ebbd7a6](https://github.com/lobinuxsoft/game-progress-tracker/commit/ebbd7a6b7f4572d09531bf52738da6c79b23633c)), closes [#38](https://github.com/lobinuxsoft/game-progress-tracker/issues/38)
* split drm.rs (509 lines) into drm/ sub-modules ([cc2f1bd](https://github.com/lobinuxsoft/game-progress-tracker/commit/cc2f1bd1d24eb4b79578220352ad9a6169f3cccd))
* split drm.rs (509 lines) into drm/ sub-modules ([1b76c20](https://github.com/lobinuxsoft/game-progress-tracker/commit/1b76c20fefdbcf4be348526cf28ea9a638920423)), closes [#36](https://github.com/lobinuxsoft/game-progress-tracker/issues/36)
* split frontend/app.js (842 lines) into ES modules ([33b0e44](https://github.com/lobinuxsoft/game-progress-tracker/commit/33b0e44064144ec5fa9e8155802aff9bd8702a08))
* split frontend/app.js into ES modules ([3b27a6b](https://github.com/lobinuxsoft/game-progress-tracker/commit/3b27a6b922bb478fe90744e63b9d3bb648205c77)), closes [#34](https://github.com/lobinuxsoft/game-progress-tracker/issues/34)
* split lib.rs (504 lines) into commands/ sub-modules ([03dda80](https://github.com/lobinuxsoft/game-progress-tracker/commit/03dda8010e7ddd2212e9837821bdccc2b472201e))
* split lib.rs (504 lines) into commands/ sub-modules ([3e4be07](https://github.com/lobinuxsoft/game-progress-tracker/commit/3e4be07079c643865d1f61d4e3d30d8a0a0934b3)), closes [#35](https://github.com/lobinuxsoft/game-progress-tracker/issues/35)
* split steam.rs (357 lines) into steam/ sub-modules ([303db55](https://github.com/lobinuxsoft/game-progress-tracker/commit/303db553658645ef0c9bfa9a54f086d5c0d50371))
* split steam.rs (357 lines) into steam/ sub-modules ([b409e76](https://github.com/lobinuxsoft/game-progress-tracker/commit/b409e767d71e28c54d308e4d7899073a86743b96)), closes [#37](https://github.com/lobinuxsoft/game-progress-tracker/issues/37)


### Documentation

* add Cyberpunk redesign reference previews ([0f9f454](https://github.com/lobinuxsoft/game-progress-tracker/commit/0f9f4548ae743c0059c2f452ebf0c32b844d7eb3))

## [0.4.0](https://github.com/lobinuxsoft/game-progress-tracker/compare/v0.3.1...v0.4.0) (2026-04-10)


### Features

* add sorting by HLTB duration (main, extras, completionist) ([50ec6c8](https://github.com/lobinuxsoft/game-progress-tracker/commit/50ec6c858a8154f07df8378e23f414ee918ff250))
* add Steam favorites filter and HowLongToBeat duration integration ([075b2e8](https://github.com/lobinuxsoft/game-progress-tracker/commit/075b2e8ae2f2c7a9057e7d0879c638106f1306f8))
* Steam favorites filter + HowLongToBeat duration ([6a19871](https://github.com/lobinuxsoft/game-progress-tracker/commit/6a198718cd2a7dcf0853e6d59dcf334f28f870ef))
* Steam favorites, HLTB duration & sorting ([ec28e55](https://github.com/lobinuxsoft/game-progress-tracker/commit/ec28e5592dec2e60501fd094d9203373dde8d729))


### Bug Fixes

* collapse nested if to satisfy clippy collapsible_if lint ([8eafe2a](https://github.com/lobinuxsoft/game-progress-tracker/commit/8eafe2a9e6a6887577422674f502159097e38410))

## [0.3.1](https://github.com/lobinuxsoft/game-progress-tracker/compare/v0.3.0...v0.3.1) (2026-04-09)


### Bug Fixes

* auto-install dialog broken by bundled library conflicts ([911a751](https://github.com/lobinuxsoft/game-progress-tracker/commit/911a75104062eac2761e32e599389398f77d9351))
* defer LD_LIBRARY_PATH setup to prevent zenity/kdialog failure ([6c0d222](https://github.com/lobinuxsoft/game-progress-tracker/commit/6c0d22206741719731a35aad72a0391e9afe8358))

## [0.3.0](https://github.com/lobinuxsoft/game-progress-tracker/compare/v0.2.0...v0.3.0) (2026-04-09)


### Features

* display app version in footer ([9c62b97](https://github.com/lobinuxsoft/game-progress-tracker/commit/9c62b9742ff95aca4ff29d481f2e1e1dfd688a5b))
* display app version in footer ([0a77af5](https://github.com/lobinuxsoft/game-progress-tracker/commit/0a77af581ba613fd41156ac94cf8902182d67c7f))
* display app version in footer ([9431211](https://github.com/lobinuxsoft/game-progress-tracker/commit/943121112758598247506209b430e0c7522dd812)), closes [#16](https://github.com/lobinuxsoft/game-progress-tracker/issues/16)


### Bug Fixes

* trigger CI only on PRs to development ([9e377e1](https://github.com/lobinuxsoft/game-progress-tracker/commit/9e377e1354b64b0214f026afebbe49cdb6110534))

## [0.2.0](https://github.com/lobinuxsoft/game-progress-tracker/compare/v0.1.0...v0.2.0) (2026-04-09)


### Features

* add CI, release and release-please workflows ([2a635d1](https://github.com/lobinuxsoft/game-progress-tracker/commit/2a635d1001e9845210a5f679adb4f3f517f2a643))
* add CI, release and release-please workflows ([04a329b](https://github.com/lobinuxsoft/game-progress-tracker/commit/04a329b42381b98df3f3f37314f24886aaa6b60a)), closes [#9](https://github.com/lobinuxsoft/game-progress-tracker/issues/9)
* add Non-Steam games tab reading from shortcuts.vdf ([a34031e](https://github.com/lobinuxsoft/game-progress-tracker/commit/a34031eec9bc2dd9279dcc264e49ce9da3115ee9))
* add Windows build support ([f251d08](https://github.com/lobinuxsoft/game-progress-tracker/commit/f251d085f2ea95ef0d3c96a1269327bf01ad0395))
* add Windows build support ([a3b9394](https://github.com/lobinuxsoft/game-progress-tracker/commit/a3b939480cbe54ba85afb0989cab56af5c620e72)), closes [#10](https://github.com/lobinuxsoft/game-progress-tracker/issues/10)
* AppImage build with self-installing AppRun and app icon ([8433b37](https://github.com/lobinuxsoft/game-progress-tracker/commit/8433b3746cf9f4b1c551f6025765f6d918aec4af))
* async card/badge loading with full badge images ([86edfd7](https://github.com/lobinuxsoft/game-progress-tracker/commit/86edfd79eca6b33c75c59ea158f1165393605f4d))
* async card/badge loading with full badge images and global loading overlay ([400271e](https://github.com/lobinuxsoft/game-progress-tracker/commit/400271e03ce69a16da4c3eda5c36b1ff1b5b87ef))
* CI/CD pipelines, robust AppImage bundling, and Windows support ([955d970](https://github.com/lobinuxsoft/game-progress-tracker/commit/955d970d57a89154a17a09303a1415df22df325c))
* configurable Steam API Key and Steam ID ([5aae5a3](https://github.com/lobinuxsoft/game-progress-tracker/commit/5aae5a352b128118eee5241b5060207711499f94))
* configurable Steam API Key and Steam ID via Settings tab ([42ac1bd](https://github.com/lobinuxsoft/game-progress-tracker/commit/42ac1bddbe45320575d1bd4c3845ea1c95e7e536)), closes [#1](https://github.com/lobinuxsoft/game-progress-tracker/issues/1)
* game detail modal with achievement progress ([338ae65](https://github.com/lobinuxsoft/game-progress-tracker/commit/338ae656159c17080477977ce4f589a4e23373a6))
* game detail modal with achievement progress and on-demand loading ([ca7fe20](https://github.com/lobinuxsoft/game-progress-tracker/commit/ca7fe20f967717065a51a7719cbdcf32a631a075)), closes [#2](https://github.com/lobinuxsoft/game-progress-tracker/issues/2)
* game progress tracker with Steam API, achievements, cards and genres ([ffed32d](https://github.com/lobinuxsoft/game-progress-tracker/commit/ffed32d5a0540eb25745d1c730ea84ab7dd2c9db))
* robust AppImage with full library bundling ([41c48eb](https://github.com/lobinuxsoft/game-progress-tracker/commit/41c48eb80a640824ec8ce2abee43552024ee9bc2))
* robust AppImage with full library bundling ([5b59f77](https://github.com/lobinuxsoft/game-progress-tracker/commit/5b59f77f6e96126c400084ad19ec006fa7fa0328)), closes [#8](https://github.com/lobinuxsoft/game-progress-tracker/issues/8)
* trading cards and badges tab in game detail modal ([6499088](https://github.com/lobinuxsoft/game-progress-tracker/commit/6499088ba1fb80e45c17fb151c671051f867ba4b))
* trading cards and badges tab in game detail modal ([4d22c62](https://github.com/lobinuxsoft/game-progress-tracker/commit/4d22c62cd54e22bf5604b348de55b31a46009757)), closes [#5](https://github.com/lobinuxsoft/game-progress-tracker/issues/5)


### Bug Fixes

* apply cargo fmt and revert invalid exe bundle target ([8643ab9](https://github.com/lobinuxsoft/game-progress-tracker/commit/8643ab9029985c632d63ec36528650d13fa0afb3))
* escape spaces in .desktop Exec path for AppImage ([03ad6e3](https://github.com/lobinuxsoft/game-progress-tracker/commit/03ad6e301fbbcbdded8bfdb04020d1b7b4cf18af))
* remove CI push trigger on main to avoid duplicate runs ([5670387](https://github.com/lobinuxsoft/game-progress-tracker/commit/5670387cef2f0a86892db7eccb545d106377b72a))
* resolve clippy warnings (collapsible_if, unused variable) ([36580dc](https://github.com/lobinuxsoft/game-progress-tracker/commit/36580dce4b0980097d1f4758fe2bfc1ed15a50de))
* trigger CI on PRs to both development and main ([dd35ebc](https://github.com/lobinuxsoft/game-progress-tracker/commit/dd35ebcc626ab434aa8d02c0350462f5723f679f))
