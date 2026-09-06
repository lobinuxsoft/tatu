# Changelog

## [0.13.2](https://github.com/lobinuxsoft/tatu/compare/v0.13.1...v0.13.2) (2026-09-06)


### Bug Fixes

* **cartridge:** recover from an orphaned steam_api backup ([feae4c4](https://github.com/lobinuxsoft/tatu/commit/feae4c4573036a031c5b70ea9f60686fd79ca575))
* **cartridge:** recover from an orphaned steam_api backup ([00471a4](https://github.com/lobinuxsoft/tatu/commit/00471a475edf860c81c977f28da35ed08d467df8))

## [0.13.1](https://github.com/lobinuxsoft/tatu/compare/v0.13.0...v0.13.1) (2026-09-06)


### Bug Fixes

* **cartridge:** force IPv4 for runtime downloads ([13510dc](https://github.com/lobinuxsoft/tatu/commit/13510dc89a54072da9a777c1f0c95940cde92d32))
* **cartridge:** force IPv4 for runtime downloads ([d1c5f39](https://github.com/lobinuxsoft/tatu/commit/d1c5f3955ac74bfb7e61af3e8dc94b6b90d9943d))

## [0.13.0](https://github.com/lobinuxsoft/tatu/compare/v0.12.2...v0.13.0) (2026-09-06)


### Features

* **launcher:** copy the game to local disk before launching it ([07d0388](https://github.com/lobinuxsoft/tatu/commit/07d03887373ab9ea7e5867d6e6ba54c023f2c0a4))
* **launcher:** copy the game to local disk before launching it ([2b06d46](https://github.com/lobinuxsoft/tatu/commit/2b06d46cf026e06712cfec7bd9bef11f0c9e69e1))

## [0.12.2](https://github.com/lobinuxsoft/tatu/compare/v0.12.1...v0.12.2) (2026-09-06)


### Bug Fixes

* **drm:** scope launcher-vendor DRM detection to the Steam row only ([bd14981](https://github.com/lobinuxsoft/tatu/commit/bd149811123eb769dc17be08b277f35399d9630a))
* **drm:** scope launcher-vendor DRM detection to the Steam row only ([c9750e9](https://github.com/lobinuxsoft/tatu/commit/c9750e9f592b50650ced2caaa1e83bed1279a3d1))

## [0.12.1](https://github.com/lobinuxsoft/tatu/compare/v0.12.0...v0.12.1) (2026-09-06)


### Bug Fixes

* **ci:** copy export templates into the container's real $HOME ([554109e](https://github.com/lobinuxsoft/tatu/commit/554109e7f4230238f668bdd95a829becd80c2f27))
* **ci:** copy export templates into the container's real $HOME ([b800951](https://github.com/lobinuxsoft/tatu/commit/b8009518e855f182a0b191754640c2ca1debc469))
* **ci:** set the executable bit on launcher/export.sh ([bd2d243](https://github.com/lobinuxsoft/tatu/commit/bd2d2438da24cf9d7fe6125a695fad703289461f))
* **ci:** set the executable bit on launcher/export.sh ([a496271](https://github.com/lobinuxsoft/tatu/commit/a496271da4413f6a526240c7659a287a3c5935d8))
* **ci:** ship Goldberg and the Godot launcher in real releases ([d6b286c](https://github.com/lobinuxsoft/tatu/commit/d6b286cfd27946d948144893b6261046ba128a47))
* **ci:** ship Goldberg and the Godot launcher in real releases ([6957b2b](https://github.com/lobinuxsoft/tatu/commit/6957b2bb8879a7e9b984966f820d888974cc2990)), closes [#285](https://github.com/lobinuxsoft/tatu/issues/285)

## [0.12.0](https://github.com/lobinuxsoft/tatu/compare/v0.11.0...v0.12.0) (2026-09-05)


### Features

* **cartridge:** let the user reorder games in the launcher ([a346268](https://github.com/lobinuxsoft/tatu/commit/a3462683422ddc9074125bc4eeff5394fec7eb86))
* **cartridge:** let the user reorder games in the launcher ([479faee](https://github.com/lobinuxsoft/tatu/commit/479faeeb04fbc3c40f985f7ff9c98db487f4757a))
* **library:** mark Steam/GOG games as installed on a cartridge ([1b28c1b](https://github.com/lobinuxsoft/tatu/commit/1b28c1b3718b335fbb1bc5a66812a8ea3fedeb46))
* **library:** mark Steam/GOG games as installed on a cartridge ([aac7952](https://github.com/lobinuxsoft/tatu/commit/aac795215a0653a4efa27fa65ba3b42faba5e693))


### Bug Fixes

* **cartridge:** don't refuse Goldberg injection for a SteamStub-wrapped entry point ([4c90536](https://github.com/lobinuxsoft/tatu/commit/4c90536956e9c40393f6d6b99a967bbe6286f6fd))
* **cartridge:** don't refuse Goldberg injection for a SteamStub-wrapped entry point ([00dba4c](https://github.com/lobinuxsoft/tatu/commit/00dba4c69cfe843d3bc5e7da32796abd75fa1cc0))
* **cartridge:** every game's saves point at its real Steam location ([9ac4d7b](https://github.com/lobinuxsoft/tatu/commit/9ac4d7b1848274b1515bea4ce3fb86b0beed13b3))
* **cartridge:** point Goldberg's local_save_path at real Steam userdata ([a068bd7](https://github.com/lobinuxsoft/tatu/commit/a068bd7ce851838104bebb3b7b27134f2d72084b))
* **launcher:** reuse a game's real compatdata prefix across libraries ([5a15bf7](https://github.com/lobinuxsoft/tatu/commit/5a15bf7adf98c6375982d01aae071f9507882d9d))

## [0.11.0](https://github.com/lobinuxsoft/tatu/compare/v0.10.0...v0.11.0) (2026-09-05)


### Features

* **cartridge:** resolve the standalone entry point from Steam's own appinfo.vdf ([a20de04](https://github.com/lobinuxsoft/tatu/commit/a20de04e68662ce410efc844dc059069c9c0725f))


### Bug Fixes

* **cartridge:** resolve standalone entry point from Steam's own appinfo.vdf ([fae54b3](https://github.com/lobinuxsoft/tatu/commit/fae54b325e327d1e1fb6b2d2af094f04668a2569))
* **cartridge:** surface per-app injection errors in the UI ([2a9aa8a](https://github.com/lobinuxsoft/tatu/commit/2a9aa8abd171a218cc1c7598e69f9ad606777e94))

## [0.10.0](https://github.com/lobinuxsoft/tatu/compare/v0.9.0...v0.10.0) (2026-09-04)


### Features

* **cartridge:** "install to a cartridge" modal in the game detail view ([8e26eb8](https://github.com/lobinuxsoft/tatu/commit/8e26eb858c0433e697a7583771b4105bafb8ecef))
* **cartridge:** "install to a cartridge" modal in the game detail view ([93e936b](https://github.com/lobinuxsoft/tatu/commit/93e936b2c636c33b00695123b53482ffb873b68b))
* **cartridge:** auto re-classify and inject Goldberg in Preparar launcher ([eac0481](https://github.com/lobinuxsoft/tatu/commit/eac0481b64ecf4442a1fc8ab779c824cbfcc06a1))
* **cartridge:** bundle Linux runtime (umu-run + Proton + Steam Linux Runtime) ([6e6bd5a](https://github.com/lobinuxsoft/tatu/commit/6e6bd5ad74d18294b349a864c65c32cdb703ca2d))
* **cartridge:** cache SteamGridDB cover art onto the cartridge ([ef3dee7](https://github.com/lobinuxsoft/tatu/commit/ef3dee7c2655671e39acb7c9e18fb1f7351022f5))
* **cartridge:** cache SteamGridDB cover art onto the cartridge ([722ea3e](https://github.com/lobinuxsoft/tatu/commit/722ea3e54f8e9fc8f0385e4188303b4b3ac2d56c)), closes [#205](https://github.com/lobinuxsoft/tatu/issues/205)
* **cartridge:** dedicated format/reformat entry point in the Cartucho tab ([934fe7d](https://github.com/lobinuxsoft/tatu/commit/934fe7db29c8e8b89a3a1f865aff86ab4f5b343d))
* **cartridge:** detect and refuse read-only drives ([447cf54](https://github.com/lobinuxsoft/tatu/commit/447cf541b5b7da6457bf65c895fb6f8fae8149bf))
* **cartridge:** disk usage breakdown bar chart in the Cartucho tab ([93451bc](https://github.com/lobinuxsoft/tatu/commit/93451bc2e0d5d70f0776975b60aebcdb1ef7f26c))
* **cartridge:** disk usage breakdown bar chart in the Cartucho tab ([16e5f8c](https://github.com/lobinuxsoft/tatu/commit/16e5f8cc469ec514959f882cf852d832d889866a))
* **cartridge:** enumerate removable drives + detect an existing cartridge ([a58a974](https://github.com/lobinuxsoft/tatu/commit/a58a9743f52d24d60970cdb6ecaa6bd2ec1ef8d6))
* **cartridge:** enumerate removable drives + detect an existing cartridge ([2b1c9ec](https://github.com/lobinuxsoft/tatu/commit/2b1c9ecc775f7bf78580d15c693e552b1e30ff1f))
* **cartridge:** format a drive as a cartridge (destructive, own safety bar) ([8c99eb6](https://github.com/lobinuxsoft/tatu/commit/8c99eb6a8cfd017bed74a7b18b33453402fb6c3c))
* **cartridge:** format a drive as a cartridge (destructive, own safety bar) ([410f0f1](https://github.com/lobinuxsoft/tatu/commit/410f0f1eea9c54679e30a8e559a26e0fce3b6e20))
* **cartridge:** Goldberg injection for Steam-wrapper-only games ([92516aa](https://github.com/lobinuxsoft/tatu/commit/92516aa7c3a4a40fcbee5d0d32f7f34e31fc76a2))
* **cartridge:** Goldberg injection for Steam-wrapper-only games ([2fdf2a6](https://github.com/lobinuxsoft/tatu/commit/2fdf2a66b3f3919485c9986fd4c5b2306806a5f4))
* **cartridge:** Linux execution via umu-run + Proton ([#206](https://github.com/lobinuxsoft/tatu/issues/206)) ([51d5573](https://github.com/lobinuxsoft/tatu/commit/51d55737fb4c3c3701beced52986347dd7503415))
* **cartridge:** register as a Steam library + trigger/track a standard install ([c9e4c88](https://github.com/lobinuxsoft/tatu/commit/c9e4c88aa61eae2f97b5beb24ebef539bf732d10))
* **cartridge:** register as a Steam library + trigger/track a standard install ([1119c3f](https://github.com/lobinuxsoft/tatu/commit/1119c3f9c57bb8230002279f3185e78ca4dd0b90))
* **drm:** fingerprint installed files when classification is Unknown ([92716a5](https://github.com/lobinuxsoft/tatu/commit/92716a524139bdc38dfd88408fe2c26ec109c06d))
* **drm:** local-file fingerprinting + automatic Goldberg re-injection ([4e71039](https://github.com/lobinuxsoft/tatu/commit/4e71039dadccaa369fe3fe3d3e5166763d21cbe0))
* **drm:** query GOG's own catalog instead of inferring from PCGamingWiki ([a35b26f](https://github.com/lobinuxsoft/tatu/commit/a35b26f78e63fbf4f3489c62992d8819db17632d))
* **drm:** query GOG's own catalog instead of only inferring from PCGW ([c635672](https://github.com/lobinuxsoft/tatu/commit/c63567228b87592ff6817251969f242f6e1989c2))
* **gog:** account login, owned-games library, shared detail template ([ff1da56](https://github.com/lobinuxsoft/tatu/commit/ff1da56af64d0fc641b3a0c02a97cad583e7b5eb))
* **gog:** account login, owned-games library, shared detail template ([cce7019](https://github.com/lobinuxsoft/tatu/commit/cce7019dbb4dff2ef636b5c68f3f023392dba4a3))
* **gog:** content-system v2 protocol reader ([9c1dd20](https://github.com/lobinuxsoft/tatu/commit/9c1dd20a7d18cf0ad5d82a90fb1290bf49c0649a))
* **gog:** implement content-system v2 protocol reader ([0a97bf2](https://github.com/lobinuxsoft/tatu/commit/0a97bf265127f281671cab0199a1b9bbe3515a36))
* **gog:** install to cartridge - marker integration, resilient downloads, launcher UI ([be9bc2a](https://github.com/lobinuxsoft/tatu/commit/be9bc2a2e1ce6b2f85f35ec45cffa6588e6540d1))
* **gog:** install-to-cartridge UI - size preview, real cancel, GOG art in Preparar launcher ([5ba04e9](https://github.com/lobinuxsoft/tatu/commit/5ba04e9de7e53a81203f4b594325b3c5594ced04))
* **gog:** multi-file depot download orchestration ([40efb5c](https://github.com/lobinuxsoft/tatu/commit/40efb5c78043659b7307db152fd9ed546e3f6477))
* **gog:** register downloaded games on the cartridge marker ([298690d](https://github.com/lobinuxsoft/tatu/commit/298690da03e89f61f7de96300ef89a2cfd336956))
* **launcher:** --cartridge-root dev override for smoke-testing ([bf71290](https://github.com/lobinuxsoft/tatu/commit/bf7129007328ab37d5ad556841a4aff11dcc3f71))
* **launcher:** --cartridge-root dev override for smoke-testing ([#206](https://github.com/lobinuxsoft/tatu/issues/206)) ([170c550](https://github.com/lobinuxsoft/tatu/commit/170c5506ea745b06d620aa9c46d09a06fe93dd24))
* **launcher:** add an editor-only test cartridge fixture ([68f4565](https://github.com/lobinuxsoft/tatu/commit/68f4565ed8ad999e4952303d78b22279fd690c9f))
* **launcher:** apply Steam Non-Steam shortcut + art for GOG apps via CDP ([6b72718](https://github.com/lobinuxsoft/tatu/commit/6b727185d07b84122de3a2dfcff69d4440a75ac8))
* **launcher:** automate Steam library registration ([#208](https://github.com/lobinuxsoft/tatu/issues/208)) ([e71f39a](https://github.com/lobinuxsoft/tatu/commit/e71f39a9b1463a7c7b86bd316fc392720e706574))
* **launcher:** automate Steam library registration ([#208](https://github.com/lobinuxsoft/tatu/issues/208)) ([9c361d5](https://github.com/lobinuxsoft/tatu/commit/9c361d504599d81c94bbe733588fe8e8747a8d33))
* **launcher:** copy launcher binaries onto cartridge, add Cartucho management tab ([99c3b42](https://github.com/lobinuxsoft/tatu/commit/99c3b4228ce71bfa102b34141fb69f55cbecd08f))
* **launcher:** copy launcher binaries onto the cartridge, add cartridge management tab ([#204](https://github.com/lobinuxsoft/tatu/issues/204)) ([dbb9d77](https://github.com/lobinuxsoft/tatu/commit/dbb9d77788eb016ea4ac18f90acc93d4027ae8c1))
* **launcher:** deploy and run Goldberg-patched games via umu-run ([#206](https://github.com/lobinuxsoft/tatu/issues/206)) ([01e9d9d](https://github.com/lobinuxsoft/tatu/commit/01e9d9dd6e6942bf0af1fbad869808c44ff6194c))
* **launcher:** fetch real Steam screenshots for the gallery ([c16ccaa](https://github.com/lobinuxsoft/tatu/commit/c16ccaa10214fd3d55e57d53715d0c4ebfc4d120))
* **launcher:** fetch real Steam screenshots for the gallery ([#213](https://github.com/lobinuxsoft/tatu/issues/213)) ([72069eb](https://github.com/lobinuxsoft/tatu/commit/72069eb37c1aedea4fa3ad0760dcc156821ef59b))
* **launcher:** opt-in trailer video as cartridge background ([3bca636](https://github.com/lobinuxsoft/tatu/commit/3bca636d309aa52d63c66ac409e0979e0fe82f55))
* **launcher:** opt-in trailer video as cartridge background ([#212](https://github.com/lobinuxsoft/tatu/issues/212)) ([b6ba4ac](https://github.com/lobinuxsoft/tatu/commit/b6ba4acf9340fd8190740a6c1f02f79d6a0f3b72))
* **launcher:** portrait cards with rounded corners + shadow, real HBox row ([b1586e4](https://github.com/lobinuxsoft/tatu/commit/b1586e45c96c97c25e14b8956c0d7b652b086469))
* **launcher:** real fonts/icons, bounce easing, drag-to-browse, proportional UI ([94cc845](https://github.com/lobinuxsoft/tatu/commit/94cc8456a6a85eea9cec5eb3bfd265e8d260657a))
* **launcher:** real glass panels — cards blur through, not clip against ([d376b07](https://github.com/lobinuxsoft/tatu/commit/d376b07cc7e01381b3d1abc822ffd7030800ce69))
* **launcher:** rework as a Steam-Deck-style carousel ([19bd23b](https://github.com/lobinuxsoft/tatu/commit/19bd23b598250b1008da64b2ef91f13333af5c5b))
* **launcher:** scaffold the cartridge launcher (Godot, animated cards) ([29fd45d](https://github.com/lobinuxsoft/tatu/commit/29fd45df92a11882ec6a7bac17fea86c87c0c534))
* **launcher:** scaffold the cartridge launcher (Godot, animated cards) ([15fd5ee](https://github.com/lobinuxsoft/tatu/commit/15fd5ee329e27228e577dac4d0a7e5e46722aca5))
* **launcher:** screenshot gallery with gamepad-first navigation ([#213](https://github.com/lobinuxsoft/tatu/issues/213)) ([2555485](https://github.com/lobinuxsoft/tatu/commit/2555485fac8e8daf675156ea794af113de1887ef))
* **launcher:** Steam shortcut + art for GOG games via CDP ([53e78fd](https://github.com/lobinuxsoft/tatu/commit/53e78fdc975e836ebff8f202ea6209a8dba26ccb))
* **launcher:** version-check runtime, always refresh art/description, 720p trailers ([e4e198b](https://github.com/lobinuxsoft/tatu/commit/e4e198b8d755fae6f6d0ad1a8072d19d07f95b92))
* **launcher:** version-check the runtime, always refresh art/description, bump trailer to 720p ([a7c832c](https://github.com/lobinuxsoft/tatu/commit/a7c832c2555b87893f0dee6537ab42413e139def))
* **steam,gog,drm:** filter by publisher/developer, bulk detail fetch, PCGamingWiki auth ([f43ae0e](https://github.com/lobinuxsoft/tatu/commit/f43ae0e1cb3f1a5e556fa1425e21186fcaf16658))
* **ui:** visible progress bar for DRM analysis ([4910f09](https://github.com/lobinuxsoft/tatu/commit/4910f09d26c6b1ca8ba421055bea33452ccd05fb))
* **ui:** visible progress bar for DRM analysis ([1913d6b](https://github.com/lobinuxsoft/tatu/commit/1913d6b6d54545c219e92dc76aaef4fcdc92e118))


### Bug Fixes

* **cartridge,launcher:** fix silent first-run failures and dead gamepad focus ([696a4bb](https://github.com/lobinuxsoft/tatu/commit/696a4bbb103357df4b8749ab5a4844af91de673c))
* **cartridge,launcher:** fix silent first-run failures and dead gamepad focus ([a619d0a](https://github.com/lobinuxsoft/tatu/commit/a619d0a3e8ecb96bcc6b92b4de2a1c4941d8d4ac))
* **cartridge:** force Windows depot, resume installs, isolate umu storage ([4baf288](https://github.com/lobinuxsoft/tatu/commit/4baf28846952a086a6e711074d548ea4d40c009f))
* **cartridge:** format the whole disk, not just the existing partition ([f62ef33](https://github.com/lobinuxsoft/tatu/commit/f62ef336e6ccecf64ef44b676e1ebc654ccd17f9))
* **cartridge:** keep Proton's wineprefix working off a cartridge ([9a4aede](https://github.com/lobinuxsoft/tatu/commit/9a4aede5689938c60f2094bbd172d6b11bdd76a5))
* **cartridge:** keep standalone Goldberg saves on the real account and prefix ([49bd054](https://github.com/lobinuxsoft/tatu/commit/49bd054db6e9b782d5f241cc28d70134fc3d95bc))
* **cartridge:** make exe-picking cross-platform, unbreaking Windows CI ([ea4957d](https://github.com/lobinuxsoft/tatu/commit/ea4957d71fdd68d6761594a4cdde4f4ced993f35))
* **cartridge:** mount an already-formatted drive that shows unmounted ([f83a6ac](https://github.com/lobinuxsoft/tatu/commit/f83a6ac8fb641841721731b0b76c1cb70242c1ec))
* **cartridge:** mount, resume, force-Windows-depot, isolate umu (found live smoke-testing [#206](https://github.com/lobinuxsoft/tatu/issues/206)) ([2f9fe11](https://github.com/lobinuxsoft/tatu/commit/2f9fe114b8ef9a08539ffb0b7a739e3c3e42b16d))
* **cartridge:** NTFS symlinks off a cartridge, whole-disk format, dedicated format entry point ([2b7630f](https://github.com/lobinuxsoft/tatu/commit/2b7630f7e8bd11e515088ff20dc74e831208731d))
* **cartridge:** standalone Goldberg saves stay on the real account and Steam Cloud ([709b93b](https://github.com/lobinuxsoft/tatu/commit/709b93b9880839a8d51cd3fb82ea80171854b153))
* **cartridge:** sync marker with Steam-installed apps, slim detail-view IPC ([f31b523](https://github.com/lobinuxsoft/tatu/commit/f31b523b51345cd97422f160e358a515d46d7c90))
* **cartridge:** sync marker with Steam-installed apps, slim detail-view IPC ([6821427](https://github.com/lobinuxsoft/tatu/commit/6821427ec83958c23729ac654645f36cac62dd24))
* **drm:** gate the local-file probe behind cfg(unix) ([70ebc7a](https://github.com/lobinuxsoft/tatu/commit/70ebc7a510971af3d447a7dcc708fb9ee0116c51))
* **gog:** resilient downloads - retry, larger chunks, real cancel ([1a8cff2](https://github.com/lobinuxsoft/tatu/commit/1a8cff28bb7e41be066a5334930bcab1a2226a92))
* **launcher:** Add to Steam gets the same status feedback as launching ([40d015f](https://github.com/lobinuxsoft/tatu/commit/40d015f74608ee0ef2621b0109d08ea301bf874b))
* **launcher:** add window/exe icon, show feedback while launching ([#204](https://github.com/lobinuxsoft/tatu/issues/204)) ([e5e52e8](https://github.com/lobinuxsoft/tatu/commit/e5e52e843b085c597465dd6976df5c4fc63452d0))
* **launcher:** always restart Steam on Add Cartridge, stamp build info ([f31dc05](https://github.com/lobinuxsoft/tatu/commit/f31dc054ecad04d8b79b12eb3991ec826db8b6ff))
* **launcher:** always restart Steam on Add Cartridge, stamp build info ([5ff5393](https://github.com/lobinuxsoft/tatu/commit/5ff5393c32e662b37c329ad5bdeed172d9900168))
* **launcher:** always show card name, anchor actions to panel bottom ([d618ae5](https://github.com/lobinuxsoft/tatu/commit/d618ae53b1a8aba46251900c3ea7bd37bd70d149))
* **launcher:** bottom-center action bar, browsable screenshot viewer ([b7d9b06](https://github.com/lobinuxsoft/tatu/commit/b7d9b06eef2b1c1329602df37ef3621d1f807d26))
* **launcher:** cards rescale on resize, gap at panel edges, better easing ([4eb94d1](https://github.com/lobinuxsoft/tatu/commit/4eb94d1a1d85df96a2d29012fc86424bce258267))
* **launcher:** decouple Add Cartridge from the selected game ([1556487](https://github.com/lobinuxsoft/tatu/commit/155648706ddcb51d7d7bbd1f2c9a0a93a0b31a57))
* **launcher:** export.sh writes straight to the vendor path Tatu reads from ([ebdbc74](https://github.com/lobinuxsoft/tatu/commit/ebdbc74c1c5411fc332d2f7c790219ac51ee72c1))
* **launcher:** export.sh writes straight to the vendor path Tatu reads from ([e7c697e](https://github.com/lobinuxsoft/tatu/commit/e7c697ebd587f4b326eb0aa8d587bf8a12da3cb0)), closes [#252](https://github.com/lobinuxsoft/tatu/issues/252)
* **launcher:** give Add to Steam the same status feedback as launching ([814e897](https://github.com/lobinuxsoft/tatu/commit/814e89768eac1e2d84f78073a9e04c274036e206))
* **launcher:** glass background on action bar, smaller covers, smoother motion ([feab389](https://github.com/lobinuxsoft/tatu/commit/feab389d0201f4f619c6c01b2a3b2f8e53d67f05))
* **launcher:** lock to 16:9 and keep input prompts on top ([abd7b44](https://github.com/lobinuxsoft/tatu/commit/abd7b449b61405411c20d870eeb21e49f7e564f8))
* **launcher:** lock to 16:9 and keep input prompts on top ([169cc28](https://github.com/lobinuxsoft/tatu/commit/169cc28af216b2aa99255e4d87ad3818ca988ac4))
* **launcher:** main.gd failed to parse since [#257](https://github.com/lobinuxsoft/tatu/issues/257)'s revert ([41d3127](https://github.com/lobinuxsoft/tatu/commit/41d31275b7ae1b9df08f0e9fdf10cc79ecfd4bcc))
* **launcher:** main.gd failed to parse since [#257](https://github.com/lobinuxsoft/tatu/issues/257)'s revert ([7efe2f5](https://github.com/lobinuxsoft/tatu/commit/7efe2f56e03d0142051e8489632c10f6d3fcad75)), closes [#261](https://github.com/lobinuxsoft/tatu/issues/261)
* **launcher:** move action bar to bottom-center, allow browsing the viewer ([0ceb6a4](https://github.com/lobinuxsoft/tatu/commit/0ceb6a4fd6dd9aa97551bbf3244c949ed856342b))
* **launcher:** quit after launching, redesign to a 4-button A/X/Y/B layout ([d818dda](https://github.com/lobinuxsoft/tatu/commit/d818dda3f2181710c28258296676aab9690c3ffd))
* **launcher:** quit after launching, redesign to A/X/Y/B button layout ([5c29a71](https://github.com/lobinuxsoft/tatu/commit/5c29a7199078a49a752662eab380eab21d5bb0dd))
* **launcher:** revert WorkerThreadPool background loading, crashed live ([43188db](https://github.com/lobinuxsoft/tatu/commit/43188dbe69a3794c8f8a4ad520c30a2742a72ff9))
* **launcher:** revert WorkerThreadPool background loading, crashed live ([7b1c53d](https://github.com/lobinuxsoft/tatu/commit/7b1c53d25f8d04d035a201f8369efc81cb1e8721)), closes [#256](https://github.com/lobinuxsoft/tatu/issues/256)
* **launcher:** stop feeding ffmpeg the ambiguous HLS master, lock UI mid-prepare ([b51edb0](https://github.com/lobinuxsoft/tatu/commit/b51edb06741aa9f9d26782f84aa7a855e941ed76))
* **launcher:** stop feeding ffmpeg the ambiguous HLS master, lock UI mid-prepare ([ab67d6d](https://github.com/lobinuxsoft/tatu/commit/ab67d6d8f02a9e366ced76c9bd75bf0c6dcae7a7))
* **launcher:** stop swallowing cover-art load failures, gdignore the fixture ([846ec01](https://github.com/lobinuxsoft/tatu/commit/846ec01bce20fd4408c0d7c5c4379c791ccc3242))
* **launcher:** stretch the card's content box to fill the button ([774a098](https://github.com/lobinuxsoft/tatu/commit/774a09869dc1d118cccdf9d07a64688862c1a54d))
* **launcher:** wire Add to Steam button to real registration ([#208](https://github.com/lobinuxsoft/tatu/issues/208)) ([b560f10](https://github.com/lobinuxsoft/tatu/commit/b560f102d87b29c5c3ef0d94baa7074ac86b46c8))
* **launcher:** wire Add to Steam button to real registration ([#208](https://github.com/lobinuxsoft/tatu/issues/208)) ([4468405](https://github.com/lobinuxsoft/tatu/commit/44684052fcce1a941529f9aae61e56214a216923))
* loading label hidden behind carousel, trailers too large to open fast ([9a524e9](https://github.com/lobinuxsoft/tatu/commit/9a524e92f24c3e454bede0e5ff3b81e5a3010aeb))
* loading label hidden behind carousel, trailers too large to open fast ([880d0c4](https://github.com/lobinuxsoft/tatu/commit/880d0c4fc7bd41ad76bf4f27a44426dfee7c8d97))


### Performance

* **launcher:** load background screenshot/trailer off the main thread ([2d6ef6b](https://github.com/lobinuxsoft/tatu/commit/2d6ef6bb59c142fa1decec4f4dc2557fd6443033))
* **launcher:** load background screenshot/trailer off the main thread ([28a5bf4](https://github.com/lobinuxsoft/tatu/commit/28a5bf41835f8e2ec60a1ae7bd48ca459bccff7e)), closes [#254](https://github.com/lobinuxsoft/tatu/issues/254)
* **launcher:** spread cover-art decoding across frames instead of blocking startup ([7de5e8e](https://github.com/lobinuxsoft/tatu/commit/7de5e8e465b133b8d5665f438785f8662ec41c26))
* **launcher:** spread cover-art decoding across frames instead of blocking startup ([bc84bea](https://github.com/lobinuxsoft/tatu/commit/bc84bea88628d05d91d4dfe70fc7c9db2f834d2c)), closes [#249](https://github.com/lobinuxsoft/tatu/issues/249)


### Documentation

* **help:** explain the cartridge feature in "Cómo funciona" ([68488d6](https://github.com/lobinuxsoft/tatu/commit/68488d6476f6cbb0b98778fbe0688dd830cd1a69))
* **help:** explain the cartridge feature in "Cómo funciona" ([26213f2](https://github.com/lobinuxsoft/tatu/commit/26213f2de41d84bf89b87c53505e7331816decb5))

## [0.9.0](https://github.com/lobinuxsoft/tatu/compare/v0.8.0...v0.9.0) (2026-08-23)


### Features

* **cards:** 3D tilt on hover, real card proportions, capped zoom ([707972e](https://github.com/lobinuxsoft/tatu/commit/707972ee3381146f3a58f045284d94e740f549de))
* **ui:** click a card or badge to see it full size ([5f3d2aa](https://github.com/lobinuxsoft/tatu/commit/5f3d2aae679897975818f18c64dc1223fe991273))
* **ui:** give the game detail its own window (closes [#187](https://github.com/lobinuxsoft/tatu/issues/187)) ([2cd63ec](https://github.com/lobinuxsoft/tatu/commit/2cd63ec168b49328c3841fe7689602d81ee496e0))


### Bug Fixes

* **ui:** let the detail tabs use the whole window ([0598a87](https://github.com/lobinuxsoft/tatu/commit/0598a87bb9c5b036796b8a74c0234e6b83af458f))
* **ui:** links, icons, fonts, a detachable detail window, and an app that explains itself ([204190b](https://github.com/lobinuxsoft/tatu/commit/204190b19a9b1251544e11dfe284b770c3549b56))
* **ui:** stop stranding the window on external links, and explain the app ([a742894](https://github.com/lobinuxsoft/tatu/commit/a74289414c72c38cd1d98aa6e13f19d41b24a596)), closes [#180](https://github.com/lobinuxsoft/tatu/issues/180)


### Documentation

* **ui:** explain where achievements, cards, duration and DRM come from ([b53e0d4](https://github.com/lobinuxsoft/tatu/commit/b53e0d49a28bdb4d7d2b196a56d222524618a6e8)), closes [#180](https://github.com/lobinuxsoft/tatu/issues/180)

## [0.8.0](https://github.com/lobinuxsoft/tatu/compare/v0.7.0...v0.8.0) (2026-08-23)


### Features

* **windows:** build the tracker on Windows with cheats gated off ([b13da35](https://github.com/lobinuxsoft/tatu/commit/b13da35bb501bb3520e2153dd93d21e5c06614f6)), closes [#180](https://github.com/lobinuxsoft/tatu/issues/180)
* **windows:** ship a Windows build — release rename + cheats gated off ([bb8515b](https://github.com/lobinuxsoft/tatu/commit/bb8515b60645257c4f0d93261863c29b681522d2))


### Documentation

* **release:** rename shipped artifacts to Tatu and enable the Windows leg ([97546c6](https://github.com/lobinuxsoft/tatu/commit/97546c67ebdcf77bb5da96d4ebd1d1dcc76f8896)), closes [#180](https://github.com/lobinuxsoft/tatu/issues/180)

## [0.7.0](https://github.com/lobinuxsoft/tatu/compare/v0.6.0...v0.7.0) (2026-08-01)


### Features

* 'Import .CT' button + Mono table exe-binding fallback ([81f1147](https://github.com/lobinuxsoft/tatu/commit/81f114724a702a9438871f52116e6b542a692625))
* **asm:** encode far absolute memory operands rip-relative ([78c8486](https://github.com/lobinuxsoft/tatu/commit/78c84864a06779266fb0294da26bb8f61a85c47e))
* **asm:** strip jmp far/near distance hints in long mode ([d60831d](https://github.com/lobinuxsoft/tatu/commit/d60831d1ebdbe20b6f5aaa9f236dbff3ee20d05c))
* CE-style tree view for cheats panel ([#133](https://github.com/lobinuxsoft/tatu/issues/133)) ([157bcd8](https://github.com/lobinuxsoft/tatu/commit/157bcd8ff0843465019c8b9376d05483d4f80757))
* **cheat-runtime:** direct .CT loader — drop manifest JSON intermediate ([276b726](https://github.com/lobinuxsoft/tatu/commit/276b726bb82d6a94f866d2ea4e6b2be07875a031))
* **cheat-runtime:** direct .CT loader — drop manifest JSON intermediate ([#134](https://github.com/lobinuxsoft/tatu/issues/134)) ([e9a5b33](https://github.com/lobinuxsoft/tatu/commit/e9a5b338407374ba22dc1234594d5939ed946d1d))
* **cheat-runtime:** expose ct_tables_dir_for(app_id) ([6b8a50e](https://github.com/lobinuxsoft/tatu/commit/6b8a50e9237f4cf367b2615da702d274479ad556))
* **cheat-runtime:** nested ManifestFeature.children for CE tree fidelity ([56c3f4a](https://github.com/lobinuxsoft/tatu/commit/56c3f4ac87ff9fb4a670da221e8af46ad874bcf0))
* **cheats:** flag framework-dependent cheats as "needs CE" ([cffa3b4](https://github.com/lobinuxsoft/tatu/commit/cffa3b4db3a54537011d24094619e21d69cbafdd))
* **ct_import:** exe-hint fallback for Mono / minimalist tables ([0ce4568](https://github.com/lobinuxsoft/tatu/commit/0ce4568dd99f41cb87b7115bebbfcfc6cda81aec))
* **ct_import:** recursive walk preserves CheatEntries tree ([a157065](https://github.com/lobinuxsoft/tatu/commit/a1570659c9c37339d17fb736783008825878e89e))
* **engine:** implement CE-AA reassemble() statement ([94cd2e0](https://github.com/lobinuxsoft/tatu/commit/94cd2e056339b288b16df1137eacda6b3aff15f8))
* **framework:** load Lua framework tables and run their cheats from import ([3e05c90](https://github.com/lobinuxsoft/tatu/commit/3e05c907273073f423173862352bf004a153b3df))
* **frontend:** 'Import .CT' button in the cheats panel ([e26e971](https://github.com/lobinuxsoft/tatu/commit/e26e971a4bce7af0a1e1a9a1194fefd23b3a87ff))
* **frontend:** CE-style collapsible tree view for cheats panel ([087281b](https://github.com/lobinuxsoft/tatu/commit/087281b4c4bc43b5d63e655df64ada847c203a28))
* **frontend:** per-table '✗' remove button + persistent import-failure toast ([caeef1b](https://github.com/lobinuxsoft/tatu/commit/caeef1bc0af0bfe0033e33522bd679031a7be886))
* import .CT robustness (exe-hint + remove button + Tier-1 asm) ([0d7ffdf](https://github.com/lobinuxsoft/tatu/commit/0d7ffdf90a3654459fb154e36d68975ce66b4bd7))
* **lua:** embed Lua 5.4 runtime with CE memory primitives (phase 0) ([03d0221](https://github.com/lobinuxsoft/tatu/commit/03d0221d6d8593eb92475b819deac1969da19f4d))
* **lua:** run framework cheat tables natively (CE primitives, constants, stubs) ([c8727a4](https://github.com/lobinuxsoft/tatu/commit/c8727a414ada1e8d59c2c9013c5ecdc03592d6dc))
* **mono-bridge:** native-Linux TCP client for the Mono collector ([674b53d](https://github.com/lobinuxsoft/tatu/commit/674b53dc5494caf7a5da1ce096124bae2458e5d5))
* **mono-bridge:** native-Linux TCP client for the Mono collector ([f1e43b2](https://github.com/lobinuxsoft/tatu/commit/f1e43b2a985fb68edb7d427c46fd9734645997c8))
* **mono-collector:** Windows-side Mono symbol collector (CE-compatible) ([d36cce1](https://github.com/lobinuxsoft/tatu/commit/d36cce19857280e12f685f46ff3584e9cbbf2894))
* **mono-collector:** Windows-side Mono symbol collector (CE-compatible) ([f4c44aa](https://github.com/lobinuxsoft/tatu/commit/f4c44aa1010e2d9128b4a90a07e208d68e516af7))
* **mono-collector:** winhttp.dll proxy load vector ([c0cd478](https://github.com/lobinuxsoft/tatu/commit/c0cd47826e70913fdc627277a370d3596fc6430f))
* **mono-collector:** winhttp.dll proxy load vector ([847a94f](https://github.com/lobinuxsoft/tatu/commit/847a94fcfa664c980415a98604c3dddf7814faae))
* **mono:** resolve Class:Method symbols via collector at enable time ([9d96420](https://github.com/lobinuxsoft/tatu/commit/9d96420e521b8d7776d04ca8d347e07b9d6de702))
* **mono:** wire collector to executor + parser fixes for Mono injection sites ([4f575b6](https://github.com/lobinuxsoft/tatu/commit/4f575b6963edabb647b91b1d6fdd07c7b07e0392))
* native Lua framework runtime + CE-AA reassemble/rip-relative/far-jmp ([2155b2f](https://github.com/lobinuxsoft/tatu/commit/2155b2f210746b79556de0f2f2bf856b60c77dd8))
* **prereqs:** install Mono collector as winhttp.dll proxy ([cf04772](https://github.com/lobinuxsoft/tatu/commit/cf04772282ff805d16f43d438a84e7308039f18a))
* **prereqs:** install Mono collector as winhttp.dll proxy ([1e253d9](https://github.com/lobinuxsoft/tatu/commit/1e253d9e79707422838c8e74c484af6b350b5236))
* **prereqs:** REFramework auto-detect + one-click install for RE Engine games ([e7910cc](https://github.com/lobinuxsoft/tatu/commit/e7910cc06360c368987e561ad19baacac390942a))
* **prereqs:** REFramework auto-detect + one-click install for RE Engine games ([#98](https://github.com/lobinuxsoft/tatu/issues/98)) ([8f5f3a2](https://github.com/lobinuxsoft/tatu/commit/8f5f3a275c44be57ad7f99c2d95f6b5f2d1b9e9f))
* **steam:** set WINEDLLOVERRIDES launch option for Mono collector ([8751010](https://github.com/lobinuxsoft/tatu/commit/87510102233445f8902f604c6df198874b1f11ae))
* **steam:** set WINEDLLOVERRIDES launch option for Mono collector ([72246e2](https://github.com/lobinuxsoft/tatu/commit/72246e255f4f11bebc3c1a65c8834bf89fa6926a))
* **tatu-engine:** aobscan(symbol, pattern) — no-module-scope variant ([07eac1a](https://github.com/lobinuxsoft/tatu/commit/07eac1afe15924ba255625aa4832313d74a3a108))
* **tatu-engine:** aobscan(symbol, pattern) — no-module-scope variant ([0d6f377](https://github.com/lobinuxsoft/tatu/commit/0d6f3776186dda4a214e40d8788abf77105a0c4e))
* **tatu-engine:** Tier-1 asm — 'nop' + 'test' mnemonics ([c282b48](https://github.com/lobinuxsoft/tatu/commit/c282b48befd5733c6b8f2b8b7cc16d74f1a83ffd))
* **tatu-engine:** Tier-2 asm — SSE2 scalar (79% → 91.6% corpus coverage) ([3fad404](https://github.com/lobinuxsoft/tatu/commit/3fad4040e5ed8bc4abf9448858ace47d2f054012))
* **tatu-engine:** Tier-2 asm — SSE2 scalar (mov/arith/cvt) ([bd2ec90](https://github.com/lobinuxsoft/tatu/commit/bd2ec909c8d8699fc5a734faa6e100e849afd6c2))
* **tatu-engine:** Tier-3 asm — SSE2 packed + x87 + cmov + misc (91.6% → 99.7%) ([166ebc7](https://github.com/lobinuxsoft/tatu/commit/166ebc7028ec5505848325bbfb17b7c2af619245))
* **tatu-engine:** Tier-3 asm (91.6% → 99.7% corpus coverage) ([b4b8e84](https://github.com/lobinuxsoft/tatu/commit/b4b8e84e5207341703e240bb912d859e0b890704))
* **tatu-tracker:** cheat_runtime_import_ct command ([670a615](https://github.com/lobinuxsoft/tatu/commit/670a615044597c9402c26982a4608a3058208798))
* **tatu-tracker:** cheat_runtime_remove_ct + Steam exe hint in import + enable error logging ([454b6a6](https://github.com/lobinuxsoft/tatu/commit/454b6a658191aed447103f10b6678131d111241d))
* **tatu-tracker:** FeatureView tree + recursive UUID lookup ([1a266ef](https://github.com/lobinuxsoft/tatu/commit/1a266ef0324d3cf8ba4236dc86a52e22e8eac854))
* **unity:** detect Unity scripting backend (Mono / IL2CPP) ([3ff476e](https://github.com/lobinuxsoft/tatu/commit/3ff476e61bd703d62e6a365b88fae9915b1f1cb5))
* **unity:** detect Unity scripting backend (Mono / IL2CPP) ([f3f0234](https://github.com/lobinuxsoft/tatu/commit/f3f023485db0f90a1fd685a2e1f7bc08eec76b61))


### Bug Fixes

* **analysis:** stop treating hex byte literals and type casts as symbols ([3f1b304](https://github.com/lobinuxsoft/tatu/commit/3f1b304156505f077c9ca74f2c52a1b7e2311346))
* **asm:** emit jmp/call far as CE's 14-byte absolute indirect ([db85c3d](https://github.com/lobinuxsoft/tatu/commit/db85c3d26b33548f76768f53593be89368387f0d))
* **ci:** build artifacts from the workspace target dir ([431ca44](https://github.com/lobinuxsoft/tatu/commit/431ca445acea2b2862751ec7ccf11e493ea213f6))
* **ci:** build artifacts from the workspace target dir ([51a4c40](https://github.com/lobinuxsoft/tatu/commit/51a4c40e2f14d591f66a286f61ecf8f7612577b3))
* **ct_import:** fallback derive_exe to '{ Game : X.exe }' comment block ([3cfc369](https://github.com/lobinuxsoft/tatu/commit/3cfc369a55e5bc962f2abb405bc9841495817c46))
* **engine:** default codecave alloc near the last AOB scan ([7512f79](https://github.com/lobinuxsoft/tatu/commit/7512f7925fd7d3d151192f385cd14b3e52d48ea2))
* **import:** drop legacy JSON sidecar when removing a .ct ([4ab5bcf](https://github.com/lobinuxsoft/tatu/commit/4ab5bcffa5dfde778b017e845e499495a31a92cc))
* **import:** drop legacy JSON sidecar when removing a .ct ([4638954](https://github.com/lobinuxsoft/tatu/commit/463895431d6c248e06913d3c966cc2e8adab1542))
* make CE-AA cheat tables work natively against Proton games + flag framework-dependent cheats ([80763a4](https://github.com/lobinuxsoft/tatu/commit/80763a4bafa5aa68ad02a4dd182100ae66d1b8ea))
* **parser:** support Mono descriptors, hex offsets, comment-safe headers ([4821ee7](https://github.com/lobinuxsoft/tatu/commit/4821ee71b35502c0d5deaae5267db3af88d70369))
* **runtime:** make CE-AA cheat tables work against Proton games ([72fa0e4](https://github.com/lobinuxsoft/tatu/commit/72fa0e4ec3eceed0e2fdd23cea3c7f239bbad68a))
* **unity:** detect modern Unity layouts (data.unity3d, runtime at game root) ([f4f2c8c](https://github.com/lobinuxsoft/tatu/commit/f4f2c8c9f8a01ad34a456da7756849ea54aef135))
* **unity:** detect modern Unity layouts (data.unity3d, runtime at game root) ([fc694b7](https://github.com/lobinuxsoft/tatu/commit/fc694b7fcd06c4a269648dd0c1834ec8c826eba7))


### Tests

* **cheat-runtime:** table-driven AA regression suite ([f011adc](https://github.com/lobinuxsoft/tatu/commit/f011adc207453db712abf1d0c8c0f33fcc494f71))
* **cheat-runtime:** table-driven AA regression suite (closes [#136](https://github.com/lobinuxsoft/tatu/issues/136)) ([335c5fd](https://github.com/lobinuxsoft/tatu/commit/335c5fd5052cf6582f0c00cad3381d1bbc3eeb83))

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
