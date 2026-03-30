# React 开发者上手 Yew 指南

面向熟悉 React hooks 的开发者，直接对比两者写法，跳过基础概念。

## 环境准备

```bash
# 安装 Rust wasm target
rustup target add wasm32-unknown-unknown

# 安装 trunk（类比 Vite）
cargo install trunk

# 新建项目
cargo new --lib frontend
cd frontend

# Cargo.toml 添加依赖
```

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
yew = { version = "0.21", features = ["csr"] }
yew-router = "0.18"
gloo = "0.11"          # 浏览器 API 封装（fetch、storage、events）
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

```bash
trunk serve   # 启动开发服务器，访问 localhost:8080
```

---

## html! 宏基础

`html!` 是 Yew 的模板宏，作用等同于 JSX。

### 基本规则

```rust
// 1. 单个根元素（同 React，多个元素需要用 <> 包裹）
html! {
    <>
        <h1>{"Title"}</h1>
        <p>{"Content"}</p>
    </>
}

// 2. 字符串必须用 {} 包裹
html! { <p>{"hello"}</p> }       // ✅
html! { <p>hello</p> }           // ❌ 编译错误

// 3. 变量、表达式用 {} 包裹
let name = "Alice";
html! { <p>{name}</p> }
html! { <p>{1 + 1}</p> }

// 4. 所有标签必须闭合
html! { <br /> }                 // ✅
html! { <br> }                   // ❌
```

### class 和 style

```rust
// class（注意不是 className）
html! { <div class="container flex">{"content"}</div> }

// 动态 class
let active = true;
html! { <div class={if active { "btn active" } else { "btn" }}>{"..."}</div> }

// classes! 宏（条件组合多个 class）
use yew::classes;
html! { <div class={classes!("btn", active.then_some("active"))}>{"..."}</div> }

// inline style
html! { <div style="color: red; font-size: 14px">{"..."}</div> }
```

### 属性

```rust
// 普通属性
html! { <input type="text" placeholder="search" /> }

// 布尔属性
html! { <input disabled={true} /> }
html! { <input disabled={is_disabled} /> }

// 动态属性
let href = "/devices";
html! { <a href={href}>{"link"}</a> }

// Option 属性：None 时该属性不渲染
let title: Option<&str> = Some("tooltip");
html! { <span title={title}>{"hover me"}</span> }
```

### 事件

```rust
// 内联回调
html! {
    <button onclick={Callback::from(|_: MouseEvent| {
        web_sys::console::log_1(&"clicked".into());
    })}>
        {"Click"}
    </button>
}

// 提前定义回调（推荐）
let onclick = Callback::from(|_: MouseEvent| {
    // handle click
});
html! { <button {onclick}>{"Click"}</button> }
// 变量名和属性名相同时可以简写 {onclick}，等同于 onclick={onclick}
```

### 条件与列表

```rust
// 条件渲染
html! {
    <>
        { if loading { html! { <p>{"Loading..."}</p> } } else { html! {} } }
        { if let Some(err) = &error { html! { <p>{err}</p> } } else { html! {} } }
    </>
}

// 列表渲染
let items = vec!["a", "b", "c"];
html! {
    <ul>
        { for items.iter().map(|item| html! {
            <li key={*item}>{*item}</li>
        })}
    </ul>
}
```

### 嵌套组件

```rust
// 传 props
html! { <DeviceCard id="board-01" label="Dev Board" online={true} /> }

// 字符串字面量属性可省略 {}，其他类型必须加 {}
html! { <Comp name="Alice" count={42} flag={true} /> }

// 变量名与属性名相同时可简写
let id = "board-01".to_string();
html! { <DeviceCard {id} label="Dev Board" online={true} /> }
```

---

## 组件

### 函数组件

```rust
// React
function Hello() {
  return <h1>Hello</h1>
}

// Yew
#[function_component]
fn Hello() -> Html {
    html! { <h1>{"Hello"}</h1> }
}
```

> **注意**：html! 里的字符串字面量必须用 `{}` 包裹。

### Props

```rust
// React
function Greeting({ name, count }: { name: string; count: number }) {
  return <p>{name}: {count}</p>
}

// Yew
#[derive(Properties, PartialEq)]
struct GreetingProps {
    pub name: String,
    pub count: u32,
}

#[function_component]
fn Greeting(props: &GreetingProps) -> Html {
    html! { <p>{&props.name}{": "}{props.count}</p> }
}

// 使用
html! { <Greeting name="Alice" count={42} /> }
```

> **注意**：Props 必须实现 `PartialEq`；非字符串字面量属性值用 `{}` 包裹。

### 可选 Props

```rust
// React
function Card({ title, subtitle }: { title: string; subtitle?: string }) {}

// Yew
#[derive(Properties, PartialEq)]
struct CardProps {
    pub title: String,
    #[prop_or_default]
    pub subtitle: Option<String>,
}
```

### Children

```rust
// React
function Panel({ children }: { children: React.ReactNode }) {
  return <div className="panel">{children}</div>
}

// Yew
#[derive(Properties, PartialEq)]
struct PanelProps {
    pub children: Children,
}

#[function_component]
fn Panel(props: &PanelProps) -> Html {
    html! {
        <div class="panel">
            { for props.children.iter() }
        </div>
    }
}
```

---

## Hooks

### useState → use_state

```rust
// React
const [count, setCount] = useState(0)
const increment = () => setCount(c => c + 1)

// Yew
let count = use_state(|| 0_u32);

let increment = {
    let count = count.clone();
    Callback::from(move |_: MouseEvent| count.set(*count + 1))
};

html! { <button onclick={increment}>{*count}</button> }
```

> **关键差异**：
> - `use_state` 返回 `UseStateHandle<T>`，读取值用 `*count`（解引用）
> - 闭包捕获前需要 `.clone()`
> - 更新函数是 `.set(value)`，没有函数式更新（`c => c + 1`），需先读取再计算

### useEffect → use_effect_with

```rust
// React
useEffect(() => {
  fetchUser(id).then(setUser)
}, [id])

// Yew
use_effect_with(id.clone(), {
    let user = user.clone();
    move |id: &u32| {
        let id = *id;
        wasm_bindgen_futures::spawn_local(async move {
            let fetched = fetch_user(id).await;
            user.set(fetched);
        });
    }
});
```

> **关键差异**：
> - 依赖项是第一个参数，回调是第二个（顺序相反）
> - 异步操作需要用 `spawn_local` 包裹
> - 无返回值的 cleanup 函数（Yew 0.21 支持返回 `impl FnOnce()`）

### useEffect（无依赖，只跑一次）

```rust
// React
useEffect(() => { init() }, [])

// Yew
use_effect_with((), move |_| {
    // 只在 mount 时执行一次
});
```

### useRef → use_node_ref

```rust
// React
const inputRef = useRef<HTMLInputElement>(null)
<input ref={inputRef} />
inputRef.current?.focus()

// Yew
let input_ref = use_node_ref();

let focus = {
    let input_ref = input_ref.clone();
    Callback::from(move |_: MouseEvent| {
        if let Some(input) = input_ref.cast::<web_sys::HtmlInputElement>() {
            input.focus().unwrap();
        }
    })
};

html! { <input ref={input_ref.clone()} /> }
```

### useMemo → use_memo

```rust
// React
const sorted = useMemo(() => [...items].sort(), [items])

// Yew
let sorted = use_memo(items.clone(), |items| {
    let mut v = items.clone();
    v.sort();
    v
});
```

### useCallback → use_callback

```rust
// React
const handleClick = useCallback((id: string) => {
  onSelect(id)
}, [onSelect])

// Yew
let handle_click = use_callback(on_select.clone(), |id: String, on_select| {
    on_select.emit(id);
});
```

---

## Context

```rust
// React
const ThemeContext = createContext("light")
// Provider
<ThemeContext.Provider value="dark">{children}</ThemeContext.Provider>
// Consumer
const theme = useContext(ThemeContext)

// Yew
// 1. 定义 context 类型（必须实现 PartialEq + Clone）
#[derive(Clone, PartialEq)]
struct Theme(String);

// 2. Provider
html! {
    <ContextProvider<Theme> context={Theme("dark".into())}>
        { for props.children.iter() }
    </ContextProvider<Theme>>
}

// 3. Consumer
#[function_component]
fn ThemedButton() -> Html {
    let theme = use_context::<Theme>().expect("no theme context");
    html! { <button class={theme.0.clone()}>{"Click"}</button> }
}
```

---

## 路由（yew-router）

```rust
// React Router v6
<Routes>
  <Route path="/" element={<Home />} />
  <Route path="/devices/:id" element={<DeviceDetail />} />
</Routes>

// yew-router
#[derive(Routable, PartialEq, Clone)]
enum Route {
    #[at("/")]
    Home,
    #[at("/devices/:id")]
    DeviceDetail { id: String },
    #[not_found]
    #[at("/404")]
    NotFound,
}

fn switch(route: Route) -> Html {
    match route {
        Route::Home => html! { <Home /> },
        Route::DeviceDetail { id } => html! { <DeviceDetail {id} /> },
        Route::NotFound => html! { <h1>{"404"}</h1> },
    }
}

#[function_component]
fn App() -> Html {
    html! {
        <BrowserRouter>
            <Switch<Route> render={switch} />
        </BrowserRouter>
    }
}
```

### 导航

```rust
// React Router
const navigate = useNavigate()
navigate("/devices/board-01")

// yew-router
let navigator = use_navigator().unwrap();
navigator.push(&Route::DeviceDetail { id: "board-01".into() });
```

### Link

```rust
// React Router
<Link to="/devices">Devices</Link>

// yew-router
<Link<Route> to={Route::Home}>{"Devices"}</Link<Route>>
```

---

## 事件处理

```rust
// React
<button onClick={(e) => console.log(e.target)}>Click</button>
<input onChange={(e) => setValue(e.target.value)} />

// Yew
<button onclick={Callback::from(|e: MouseEvent| {
    web_sys::console::log_1(&e.target().into());
})}>{"Click"}</button>

<input oninput={Callback::from(move |e: InputEvent| {
    let input = e.target_unchecked_into::<web_sys::HtmlInputElement>();
    value.set(input.value());
})} />
```

常用事件对应：

| React | Yew |
|---|---|
| `onClick` | `onclick` |
| `onChange` / `onInput` | `oninput` |
| `onSubmit` | `onsubmit` |
| `onKeyDown` | `onkeydown` |

---

## 条件渲染

```rust
// React
{isLoading && <Spinner />}
{error ? <Error msg={error} /> : <Content />}

// Yew
{ if *is_loading { html! { <Spinner /> } } else { html! {} } }
{ if let Some(err) = &*error {
    html! { <Error msg={err.clone()} /> }
} else {
    html! { <Content /> }
}}
```

---

## 列表渲染

```rust
// React
{devices.map(d => <DeviceCard key={d.id} device={d} />)}

// Yew
{ for devices.iter().map(|d| html! {
    <DeviceCard key={d.id.clone()} device={d.clone()} />
})}
```

> **注意**：`key` 必须是 `String` 或整数，不能是引用。

---

## 异步数据获取

```rust
// React
useEffect(() => {
  fetch("/api/devices")
    .then(r => r.json())
    .then(setDevices)
}, [])

// Yew（使用 gloo::net）
use gloo::net::http::Request;

let devices = use_state(|| vec![]);

use_effect_with((), {
    let devices = devices.clone();
    move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            let fetched: Vec<Device> = Request::get("/api/devices")
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            devices.set(fetched);
        });
    }
});
```

---

## WebSocket

```rust
use gloo::net::websocket::{futures::WebSocket, Message};
use futures::{SinkExt, StreamExt};

let events = use_state(|| vec![]);

use_effect_with((), {
    let events = events.clone();
    move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            let mut ws = WebSocket::open("ws://localhost:3000/api/ws").unwrap();
            while let Some(Ok(Message::Text(msg))) = ws.next().await {
                let event: LogEvent = serde_json::from_str(&msg).unwrap();
                events.set({
                    let mut v = (*events).clone();
                    v.push(event);
                    v
                });
            }
        });
    }
});
```

---

## 常见坑

| 问题 | 原因 | 解法 |
|---|---|---|
| 闭包里用了 state 但值不更新 | 闭包捕获了旧值 | `.clone()` 后再捕获 |
| `html!` 里不能直接用 `String` | 宏要求 `&str` 或包在 `{}` 里 | `{&my_string}` 或 `{my_string.clone()}` |
| Props 修改后组件不重渲染 | `PartialEq` 实现不正确 | 确保 derive `PartialEq`，避免手动实现出错 |
| 异步函数不能直接在回调里用 | WASM 没有多线程 | 用 `spawn_local` 包裹 |
| 列表渲染没有 key 导致警告 | 同 React | 给每个元素加唯一 `key` |

---

## 推荐学习资源

- [Yew 官方文档](https://yew.rs/docs)
- [yew-router 文档](https://yew.rs/docs/concepts/router)
- [gloo 文档](https://gloo-rs.web.app) — 浏览器 API 的 Rust 封装
- [wasm-bindgen 文档](https://rustwasm.github.io/wasm-bindgen) — JS 互操作
- implfuture.dev 源码 — 本项目参考的 Yew 全栈示例
