import Cocoa

/// What a menu item does when picked; kept in `representedObject` so one selector serves every option.
final class MenuAction: NSObject {
    let run: () -> Void
    init(_ run: @escaping () -> Void) { self.run = run }
}

extension AppDelegate {
    func applyScale(_ s: CGFloat, keepOrigin: Bool) {
        scale = s
        let size = CGSize(width: designSize.width * s, height: designSize.height * s)
        var origin = panel.frame.origin
        if !keepOrigin { origin = savedOrigin() ?? defaultOrigin(for: size) }
        panel.setFrame(CGRect(origin: origin, size: size), display: true)
    }

    func defaultOrigin(for size: CGSize) -> CGPoint {
        let area = NSScreen.main?.visibleFrame ?? CGRect(x: 0, y: 0, width: 1440, height: 900)
        return CGPoint(x: area.maxX - size.width - 20, y: area.minY)
    }

    func savedOrigin() -> CGPoint? {
        guard let x = defaults.object(forKey: "originX") as? Double,
              let y = defaults.object(forKey: "originY") as? Double else { return nil }
        let p = CGPoint(x: x, y: y)
        let probe = CGRect(origin: p, size: CGSize(width: 80, height: 80))
        return NSScreen.screens.contains { $0.frame.intersects(probe) } ? p : nil
    }

    func saveOrigin() {
        defaults.set(Double(panel.frame.origin.x), forKey: "originX")
        defaults.set(Double(panel.frame.origin.y), forKey: "originY")
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        buildMenu(menu)
    }

    func showMenu(_ event: NSEvent) {
        let menu = NSMenu()
        buildMenu(menu)
        NSMenu.popUpContextMenu(menu, with: event, for: webView)
    }

    func buildMenu(_ menu: NSMenu) {
        // Off is the flat 2D sticker look; on adds soft volume shading.
        menu.addItem(toggle("立体效果（3D）", style == "3d") { $0.style = $0.style == "3d" ? "2d" : "3d" })
        menu.addItem(submenu("毛色", [("黑猫", "black"), ("奶牛猫", "tuxedo"), ("橘猫", "orange"), ("蓝灰猫", "grey"),
                                      ("白猫", "white"), ("三花猫", "calico"), ("暹罗猫", "siamese"), ("布偶猫", "ragdoll"),
                                      ("德文卷毛猫", "devon"), ("美短银虎斑", "silver"), ("狸花猫", "lihua"),
                                      ("金渐层", "golden")], selected: coat) { $0.coat = $1 })
        // Only coats whose stripes would show get the choice; black and white stay plain.
        if coat == "orange" || coat == "grey" {
            let striped = pattern == "tabby" || (pattern == "auto" && coat == "orange")
            menu.addItem(submenu("花纹", [("纯色", "solid"), ("虎斑条纹", "tabby")],
                                 selected: striped ? "tabby" : "solid") { $0.pattern = $1 })
        }
        let looks = NSMenuItem(title: "外观", action: nil, keyEquivalent: "")
        let lookMenu = NSMenu()
        lookMenu.addItem(submenu("眼睛颜色", [("跟随毛色", "auto"), ("金色", "gold"), ("绿色", "green"), ("蓝色", "blue"),
                                              ("琥珀色", "amber"), ("鸳鸯眼（一蓝一金）", "odd")],
                                 selected: eyeColor) { $0.eyeColor = $1 })
        lookMenu.addItem(submenu("眼睛款式", [("跟随毛色", "auto"), ("杏仁眼（竖瞳）", "almond"), ("圆眼（圆瞳）", "round")],
                                 selected: eyeShape) { $0.eyeShape = $1 })
        lookMenu.addItem(submenu("眼睛高光", [("无", "none"), ("经典", "classic"), ("闪亮", "sparkle")],
                                 selected: eyeShine) { $0.eyeShine = $1 })
        lookMenu.addItem(submenu("项圈", [("无", "none"), ("红色铃铛", "red"), ("蓝色铃铛", "blue"), ("粉色铃铛", "pink"),
                                          ("绿色铃铛", "green")], selected: collar) { $0.collar = $1 })
        lookMenu.addItem(toggle("腮红", blush) { $0.blush.toggle() })
        lookMenu.addItem(toggle("胡须", whiskers) { $0.whiskers.toggle() })
        looks.submenu = lookMenu
        menu.addItem(looks)
        // 特小 is exactly two thirds of 小, for a cat that perches in a corner and covers nothing.
        menu.addItem(submenu("大小", [("特小", 0.7 * 2.0 / 3.0), ("小", 0.7), ("中", 0.9), ("大", 1.1), ("特大", 1.35)] as [(String, CGFloat)],
                             selected: scale) { $0.applyScale($1, keepOrigin: true) })
        menu.addItem(submenu("多久睡着", [("30 秒", 30), ("1 分钟", 60), ("3 分钟", 180), ("10 分钟", 600), ("不睡", 0)],
                             selected: sleepAfter) { $0.sleepAfter = $1 })
        menu.addItem(toggle("隐私模式（不显示键名）", privacy) { $0.privacy.toggle() })
        menu.addItem(toggle("鼠标穿透（点击直接落到下层窗口）", clickThrough) { $0.clickThrough.toggle() })
        menu.addItem(item("调节反应…") { $0.settings.show() })
        menu.addItem(.separator())
        menu.addItem(item("回到右下角") { $0.resetPosition() })
        menu.addItem(item(panel.isVisible ? "隐藏" : "显示") { $0.toggleVisible() })
        menu.addItem(.separator())
        if !keyboardAllowed {
            menu.addItem(item("开启「输入监控」权限…") { $0.openPrivacy() })
            menu.addItem(item("授权后重新启动") { $0.relaunch() })
            menu.addItem(.separator())
        }
        menu.addItem(updateItem())
        menu.addItem(.separator())
        menu.addItem(item("退出 KeyCat") { _ in NSApp.terminate(nil) })
    }

    /// Same wording as the Windows tray. 重启 swaps the new app in on the way out (applicationWillTerminate).
    private func updateItem() -> NSMenuItem {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
        guard updater.enabled else { return note("v\(version)（不在「应用程序」文件夹里，不自动更新）") }
        switch updater.state {
        case .ready(let new): return item("重启并更新到 v\(new)") { $0.relaunch() }
        case .downloading: return note("正在下载更新…")
        case .checking: return note("正在检查更新…")
        case .current: return note("已是最新版本 v\(version)")
        case .failed: return item("检查更新失败，点击重试") { $0.updater.check() }
        case .idle: return item("检查更新…") { $0.updater.check() }
        }
    }

    private func note(_ title: String) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        i.isEnabled = false
        return i
    }

    /// Radio-style submenu. Picking an option runs `pick` and pushes the new look to the page.
    private func submenu<T: Equatable>(_ title: String, _ options: [(String, T)], selected: T,
                                       pick: @escaping (AppDelegate, T) -> Void) -> NSMenuItem {
        let parent = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        let sub = NSMenu()
        for (label, value) in options {
            let i = item(label) { app in
                pick(app, value)
                app.pushOptions()
            }
            i.state = value == selected ? .on : .off
            sub.addItem(i)
        }
        parent.submenu = sub
        return parent
    }

    private func toggle(_ title: String, _ on: Bool, flip: @escaping (AppDelegate) -> Void) -> NSMenuItem {
        let i = item(title) { app in
            flip(app)
            app.pushOptions()
        }
        i.state = on ? .on : .off
        return i
    }

    private func item(_ title: String, _ run: @escaping (AppDelegate) -> Void) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: #selector(runMenuAction(_:)), keyEquivalent: "")
        i.target = self
        i.representedObject = MenuAction { [weak self] in if let self { run(self) } }
        return i
    }

    @objc func runMenuAction(_ sender: NSMenuItem) { (sender.representedObject as? MenuAction)?.run() }

    func resetPosition() {
        defaults.removeObject(forKey: "originX")
        defaults.removeObject(forKey: "originY")
        applyScale(scale, keepOrigin: false)
    }

    func toggleVisible() {
        if panel.isVisible { panel.orderOut(nil) } else { panel.orderFrontRegardless() }
    }

    func openPrivacy() {
        CGRequestListenEventAccess()
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent") {
            NSWorkspace.shared.open(url)
        }
    }

    func relaunch() {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/bin/sh")
        // Wait for this process to exit; `open` on a running app only brings it forward.
        task.arguments = ["-c", "while kill -0 \"$1\" 2>/dev/null; do sleep 0.1; done; open \"$0\"",
                          Bundle.main.bundlePath, String(ProcessInfo.processInfo.processIdentifier)]
        try? task.run()
        NSApp.terminate(nil)
    }
}
