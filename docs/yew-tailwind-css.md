# Yew Tailwind CSS

## 目的

本文说明如何在 Rust/Yew 前端中使用 Tailwind CSS。

参考项目表明，这条路线是可行的：Tailwind 不需要 JSX，也不要求 React。只要 Tailwind 能扫描到 Yew 写在 `.rs` 文件里的 class 名，就可以正常工作。

## 核心思路

在 Yew 项目里使用 Tailwind CSS，关键不是前端框架，而是这两点：

1. Tailwind `content` 要扫描 Rust 源码
2. 页面加载的是 Tailwind 编译后的 CSS 文件，而不是源文件

也就是说，Yew 的做法与普通前端项目本质相同：

1. 写 `tailwind.config.js`
2. 写带有 `@tailwind` 指令的源 CSS
3. 编译生成最终 CSS
4. 在 HTML 中引入生成后的 CSS

## 为什么 Yew 可以用 Tailwind

Yew 的视图写在 `.rs` 文件中，例如：

```rust
html! {
    <div class="flex items-center gap-4 rounded-xl border p-4">
        {"Device Hub"}
    </div>
}
```

这些 class 名对 Tailwind 来说只是文本。

只要 `tailwind.config.js` 里的 `content` 包含 `src/**/*.rs`，Tailwind 就能扫描并生成对应样式。

## 参考做法

参考项目采用的是标准 Tailwind 用法：

### 1. 配置扫描 Rust 文件

```js
module.exports = {
  content: ["src/**/*.rs"],
  theme: {
    extend: {},
  },
  plugins: [],
};
```

这一点最重要。

如果不扫描 `.rs`，Tailwind 不会看到 Yew 模板里的 utility class，最终生成的 CSS 也就不会包含这些样式。

### 2. 使用 Tailwind 源 CSS 文件

源 CSS 文件中写：

```css
@tailwind base;
@tailwind components;
@tailwind utilities;
```

然后在这个文件下面继续写少量自定义 CSS。

例如：

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

body {
  margin: 0;
}
```

### 3. 构建出最终 CSS

浏览器加载的不是上面的源文件，而是 Tailwind 编译后的成品 CSS。

例如页面里最终引用：

```html
<link href="/css/tailwind.css" rel="stylesheet" />
```

这说明实际运行模式是：

1. 源 CSS 提供 Tailwind 指令
2. Tailwind CLI 或其它构建步骤产出最终 CSS
3. HTML 引入产物

## 对本项目的启发

如果 `device-hub/frontend` 想使用 Tailwind CSS，重点不是引入 React，也不是先换构建系统，而是先满足 Tailwind 的基本条件：

1. `tailwind.config.js` 扫描 `frontend/src/**/*.rs`
2. 增加一个 Tailwind 源 CSS 文件
3. 构建出最终 `style.css` 或 `tailwind.css`
4. 让 `frontend/index.html` 引用这个生成后的 CSS 文件

## 与 React 无关

Tailwind CSS 不是 React 专属工具。

它只关心：

1. 你在哪里写 class 名
2. 它能不能扫描到这些文件
3. 生成的 CSS 有没有被页面加载

所以在 Rust/Yew 中使用 Tailwind CSS 是完全成立的。

## 构建角色

Tailwind 本身只是一个 CSS 构建步骤。

它可以和不同工具配合：

1. Tailwind CLI
2. PostCSS
3. Vite
4. Bazel 统一编排

但这些都只是“怎么运行 Tailwind”，不是“Tailwind 能不能在 Yew 中使用”的前提条件。

对 Rust/Yew 项目来说，最本质的前提仍然是：

```js
content: ["src/**/*.rs"]
```

## 结论

在 Rust/Yew 中使用 Tailwind CSS 的方法是成立的，而且路径很清晰：

1. 扫描 `.rs` 文件
2. 编写 `@tailwind` 源 CSS
3. 构建生成最终 CSS
4. 在页面中引入生成产物

参考项目说明了这件事可以正常工作，因此本项目如果要引入 Tailwind，应该把重点放在 Rust 文件扫描和 CSS 构建接入上，而不是先关注 React、Vite 或 Bazel。
