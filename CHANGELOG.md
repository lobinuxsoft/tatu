# Changelog

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
