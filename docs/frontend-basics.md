# 前端基础概念

面向没有前端背景的开发者，理解浏览器和前端框架的工作原理。

---

## 浏览器做了什么

你在地址栏输入一个 URL，浏览器：

1. 从服务器下载 HTML 文件
2. 解析 HTML，构建 **DOM**（页面结构树）
3. 下载并解析 CSS，计算每个元素的样式
4. 下载并执行 JS（或 WASM）
5. 把结果绘制到屏幕上

之后每次用户操作（点击、输入），JS 修改 DOM，浏览器重新绘制受影响的部分。

---

## HTML — 结构

HTML 描述页面**有什么**，用标签表示。

```html
<!DOCTYPE html>
<html>
  <head>
    <title>Device Hub</title>
  </head>
  <body>
    <h1>Devices</h1>
    <p>Welcome.</p>
    <button id="my-btn">Click me</button>
  </body>
</html>
```

常用标签：

| 标签 | 用途 |
|---|---|
| `<div>` | 通用容器（块级）|
| `<span>` | 通用容器（行内）|
| `<h1>`–`<h6>` | 标题 |
| `<p>` | 段落 |
| `<a href="...">` | 链接 |
| `<button>` | 按钮 |
| `<input>` | 输入框 |
| `<ul>` / `<li>` | 无序列表 |
| `<table>` | 表格 |

---

## DOM — 页面的内存表示

浏览器解析 HTML 后，在内存里建一棵树，叫 **DOM**（Document Object Model）。

```
document
└── html
    ├── head
    │   └── title: "Device Hub"
    └── body
        ├── h1: "Devices"
        ├── p: "Welcome."
        └── button#my-btn: "Click me"
```

JS（和 WASM）操作的就是这棵树——增删节点、修改文本、改样式，浏览器自动重新绘制。

---

## CSS — 样式

CSS 描述元素**长什么样**。

```css
/* 选中所有 h1，设置颜色和字号 */
h1 {
  color: #333;
  font-size: 2rem;
}

/* 选中 class="card" 的元素 */
.card {
  border: 1px solid #ccc;
  padding: 16px;
}

/* 选中 id="my-btn" 的元素 */
#my-btn {
  background: blue;
  color: white;
}
```

现代项目通常用 **Tailwind CSS**——直接在标签上写 class，不用手写 CSS 文件：

```html
<button class="bg-blue-500 text-white px-4 py-2 rounded">
  Click me
</button>
```

---

## JS — 行为

JS 描述页面**做什么**，运行在浏览器里。

### script 标签

浏览器遇到 `<script>` 就执行里面的代码：

```html
<button id="my-btn">Click me</button>

<script>
  // 找到按钮
  const btn = document.getElementById("my-btn")

  // 绑定点击事件
  btn.addEventListener("click", function () {
    alert("clicked!")
  })
</script>
```

这就是前端"交互"的本质：**找到元素 → 监听事件 → 执行代码 → 修改 DOM**。

### 事件

用户的每个操作都是一个事件，JS 可以监听并响应：

| 事件 | 触发时机 |
|---|---|
| `click` | 点击 |
| `input` | 输入框内容变化 |
| `submit` | 表单提交 |
| `keydown` | 按下键盘 |
| `load` | 页面/资源加载完成 |

---

## 为什么需要框架

手写 DOM 操作很快变得混乱：

```js
// 手写 DOM —— 命令式，描述"怎么做"
const li = document.createElement("li")
li.textContent = device.name
li.className = device.online ? "online" : "offline"
list.appendChild(li)

// 删除时还要手动找到并移除
list.removeChild(li)
```

问题：
- 状态（数据）和 UI（DOM）手动同步，容易出错
- 代码量大，难以维护
- 没有组件化，逻辑难以复用

**React / Yew 的解法**：声明式，描述"是什么"，框架负责同步 DOM：

```rust
// Yew —— 声明式，只描述 UI 应该长什么样
html! {
    <li class={if device.online { "online" } else { "offline" }}>
        {&device.name}
    </li>
}
// 数据变了，框架自动更新 DOM，不用手动操作
```

---

## WASM — 在浏览器里运行 Rust

浏览器原生只能执行 JS。**WebAssembly（WASM）** 是一种二进制格式，浏览器也能执行，性能接近原生。

```
Rust 代码
   ↓ 编译（cargo + wasm-bindgen）
.wasm 文件
   ↓ 浏览器加载
在浏览器里执行，可以操作 DOM
```

Yew 就是利用这个机制——你写 Rust，编译成 WASM，浏览器执行 WASM 来驱动页面，不需要写任何 JS。

```html
<!-- trunk 生成的 index.html，自动注入 WASM loader -->
<script type="module">
  import init from './frontend.js';
  init();  // 加载并启动 WASM
</script>
```

你不需要手写这段，`trunk` 自动生成。

---

## 概念速查

| 概念 | 一句话 |
|---|---|
| HTML | 页面结构，描述有什么 |
| CSS | 页面样式，描述长什么样 |
| JS | 页面行为，描述做什么 |
| DOM | 浏览器对 HTML 的内存表示，JS/WASM 操作它 |
| 事件 | 用户操作触发的信号，绑定回调来响应 |
| 框架（React/Yew）| 声明式地描述 UI，自动同步 DOM |
| WASM | 浏览器能执行的二进制格式，Rust 编译目标 |
| trunk | 把 Rust 编译成 WASM 并打包为可部署网页的工具 |
