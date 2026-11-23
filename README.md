# KeyCat

一只趴在键盘上的桌面猫：你打字它伸爪按键，动鼠标它扭头看，摸头会闭眼，没人理会睡着，被烦到会用尾巴捶桌。支持 macOS 13+ 和 Windows 10+，除了从 GitHub 检查和下载更新，不联网。

![KeyCat](docs/screenshots/curious.jpg)

## 安装

**macOS**：在终端执行

```sh
curl -L -o /tmp/KeyCat-macOS.zip https://github.com/ElloboDK/KeyCat/releases/latest/download/KeyCat-macOS.zip \
  && unzip -qo /tmp/KeyCat-macOS.zip -d /Applications && open /Applications/KeyCat.app
```

首次启动需在「系统设置 → 隐私与安全性 → 输入监控」里允许 KeyCat，然后重启应用。它只读取哪个键被按下，不记录输入内容。手动从浏览器下载时，若提示「已损坏」，执行 `xattr -dr com.apple.quarantine /Applications/KeyCat.app`。装在「应用程序」文件夹里的 KeyCat 会自动更新：启动 15 秒后和之后每 6 小时检查一次，有新版就在后台下载，退出时替换，也可以在菜单里点「重启并更新」。1.0.2 及更早的版本没有这个功能，需要用上面的命令重装一次。

**Windows**：从 [Releases](https://github.com/ElloboDK/KeyCat/releases/latest) 下载 `KeyCat-Windows-Setup.exe` 运行。SmartScreen 拦截时点「更多信息 → 仍要运行」。安装包只有几 MB，猫由系统自带的 WebView2 绘制，极少数没装 WebView2 的 Win10 会在安装时自动下载。应用会自动更新，1.0.x 的旧版会自动升级过来并保留设置。

## 使用

猫住在菜单栏（Windows 是托盘）。右键猫或点图标打开菜单，可以换毛色和外观、调大小、设置多久睡着，在「调节反应…」里调整烦躁和好奇的灵敏度。按住猫身体可以拖动位置。

## 构建

```sh
sh mac/build.sh       # → build/KeyCat.app（通用二进制，需 Xcode 命令行工具）
sh mac/package.sh     # → dist/KeyCat-macOS.zip + .sha256 + latest-mac.json

cd win && npm ci
npm start             # 本机调试
npm run dist          # → win/target/release/bundle/nsis/KeyCat_<版本>_x64-setup.exe（需 Rust 和 Node.js 22+，在 Windows 上执行）
```

mac 版用 `signing/` 里自动生成的本地证书签名，这样重新编译后「输入监控」授权仍然有效。mac 自动更新只接受同一张证书签名的新版，所以发布包必须在有这个目录的机器上打，证书丢了已安装的 mac 版也收不到更新。Windows 自动更新的签名私钥也放在这里（`tauri-updater.key` 和它的密码 `tauri-updater.password`），内容同时存在仓库的 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 两个 Secret 里，公钥写在 `win/tauri.conf.json`。私钥丢了已安装的版本就收不到更新，要另外备份。不要提交这个目录。

## 发布

mac 和 Windows 共用一个 Release，两边产物都要上传，否则另一端的下载链接或自动更新会失效。

1. 把 `win/package.json` 的 `version` 和 `mac/Info.plist` 的 `CFBundleShortVersionString` 改成同一个版本，`CFBundleVersion` 加 1，合入 main。
2. 执行 `sh mac/package.sh`，推送 `vX.Y.Z` tag，再执行 `gh release create vX.Y.Z --generate-notes dist/KeyCat-macOS.zip dist/KeyCat-macOS.zip.sha256 dist/latest-mac.json`。`latest-mac.json` 是 mac 版的更新清单，mac 版靠 `CFBundleVersion` 判断新旧，所以每次发布都要加 1。
3. tag 触发 GitHub Actions，几分钟后会把 Windows 安装包、`latest.json`（新版的更新清单）和 `latest.yml`（让 1.0.x 旧版升级过来）补传到同一个 Release。
4. 同一个 Actions 最后会把整个 Release（标题、说明、全部附件）复制到 Gitee 镜像 [banunu/KeyCat](https://gitee.com/banunu/KeyCat/releases)，需要仓库的 `GITEE_TOKEN` Secret。Gitee 的仓库镜像只同步代码和 tag，不同步 Release；Gitee 会拒收未签名的 exe，所以 Windows 安装包在那边是 `KeyCat-Windows-Setup.zip`。第 2 步要在 Actions 跑完前做完，否则 mac 包赶不上，这时手动执行 `sh tools/gitee-release.sh vX.Y.Z` 补传（token 放在 `signing/gitee-token`）。

## 目录

```
mac/    macOS 外壳（Swift）：浮窗、菜单、键鼠监听、自动更新、构建/签名/打包脚本
win/    Windows 外壳（Tauri，Rust）：托盘、全局键鼠钩子、自动更新，页面由系统 WebView2 绘制
web/    两端共用的 three.js 场景：猫、键盘、动作，tuning.js 是可调参数的唯一来源
tools/  开发用渲染脚本（serve.py 起本地服务，其余脚本在内置浏览器里截图对比）
```

## 说明

这是一个从零实现的学习项目，形象和交互参考了一款付费桌面宠物应用，仅供个人使用和内部交流，不用于商业分发，也不包含对方的任何素材。three.js 为 MIT 许可（见 `web/THREE_LICENSE`），Tauri 为 MIT / Apache-2.0 双许可。
