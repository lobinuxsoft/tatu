# Changelog

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
