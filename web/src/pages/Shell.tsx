import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Link,
  Outlet,
  useNavigate,
  useRouterState,
} from "@tanstack/react-router";
import {
  Database,
  LayoutDashboard,
  GitPullRequest,
  History,
  Clock3,
  Library as LibraryIcon,
  ListChecks,
  MessagesSquare,
  Search as SearchIcon,
  Settings as SettingsIcon,
  Shapes,
  Waypoints,
} from "lucide-react";
import { api, ApiError } from "../api";
import { S } from "../i18n";
import { useKb, useKbId } from "../kb";
import { Wordmark } from "../ui";
import { KbSwitcher } from "./KbSwitcher";
import { HeaderActions } from "./HeaderActions";
import { ServerDown } from "./ServerDown";
import { useAlertEvents } from "../useAlertEvents";
import { useKbEvents } from "../useKbEvents";
import { usePageTitle } from "../useTitle";

const TABS = [
  { to: "/kb/$kbId/overview", label: S.arcadia.home, Icon: LayoutDashboard },
  { to: "/kb/$kbId/changes", label: S.arcadia.changes, Icon: GitPullRequest },
  { to: "/kb/$kbId/traces", label: S.arcadia.traces, Icon: History },
  { to: "/kb/$kbId/history", label: S.arcadia.history, Icon: Clock3 },
  // 图谱是门面，排第一；两种查询方式（Search/Ask）随后
  { to: "/kb/$kbId/graph", label: S.nav.graph, Icon: Waypoints },
  { to: "/kb/$kbId/search", label: S.nav.search, Icon: SearchIcon },
  { to: "/kb/$kbId/chat", label: S.nav.ask, Icon: MessagesSquare },
  { to: "/kb/$kbId/library", label: S.nav.library, Icon: LibraryIcon },
  { to: "/kb/$kbId/review", label: S.review.title, Icon: ListChecks },
  { to: "/kb/$kbId/ontology", label: S.ontology.title, Icon: Shapes },
  // 本体说「世界上有什么」，数据映射说「这个数在库里怎么算」——挨着放
  { to: "/kb/$kbId/mappings", label: S.mapping.title, Icon: Database },
  // 库设置与其它 tab 同为"当前知识库作用域"，并列于内容导航
  { to: "/kb/$kbId/settings", label: S.nav.settings, Icon: SettingsIcon },
] as const;

export function Shell() {
  const navigate = useNavigate();
  const kbId = useKbId();

  const me = useQuery({ queryKey: ["me"], queryFn: api.me });
  const health = useQuery({
    queryKey: ["health"],
    queryFn: api.health,
    staleTime: Infinity,
  });
  const { kb, kbs, setKb } = useKb();
  // 标题跟随当前 tab：`Graph · Utopia`；文档查看页归入 Library
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const tabLabel =
    TABS.find((t) => pathname.startsWith(t.to.replace("$kbId", kbId)))?.label ??
    (pathname.startsWith("/doc/") ? S.nav.library : undefined);
  usePageTitle(S.app.name, tabLabel);
  // 全局唯一的 KB 事件流连接：文档/审核状态实时刷新（替轮询）
  useKbEvents(kb?.id);
  // 告警流是全局的：角标跨库，而系统级告警根本没有库
  useAlertEvents();

  // 未登录就去登录页。**副作用要在 effect 里**，理由见下面 401 那一支
  const unauthorized =
    me.isError && me.error instanceof ApiError && me.error.status === 401;
  useEffect(() => {
    if (unauthorized) navigate({ to: "/login" });
  }, [unauthorized, navigate]);

  if (me.isPending) {
    return (
      <div className="min-h-screen flex items-center justify-center text-ink-3 text-body">
        {S.nav.loading}
      </div>
    );
  }

  if (me.isError) {
    // **跳转在 effect 里做，不在渲染里。** 渲染期间调 `navigate` 是在别人渲染
    // 的过程中改路由器的状态，React 会常驻一条「Cannot update a component
    // while rendering a different component」的警告。今天不出错，但它是
    // 「渲染顺序依赖」的味道——改布局时最容易在这种地方变成真 bug
    if (me.error instanceof ApiError && me.error.status === 401) {
      return null;
    }
    return <ServerDown />;
  }

  return (
    <div className="arc-shell">
      <aside className="arc-sidebar">
        <div className="arc-brand">
          <Wordmark className="text-display" />
          <span>01</span>
        </div>
        <p className="arc-eyebrow">{S.arcadia.workspace}</p>
        <div className="arc-kb-switch">
          <KbSwitcher kb={kb} kbs={kbs} onChange={setKb} />
        </div>
        <nav aria-label={S.arcadia.workspace}>
          {TABS.map(({ to, label, Icon }, i) => (
            <div key={to}>
              {(i === 0 || i === 4 || i === 8) && (
                <p className="arc-nav-group">
                  {i === 0
                    ? S.arcadia.governance
                    : i === 4
                      ? S.arcadia.knowledge
                      : S.arcadia.setup}
                </p>
              )}
              <Link
                to={to}
                params={{ kbId }}
                className="arc-nav-item"
                activeProps={{ className: "arc-nav-item is-active" }}
              >
                <Icon size={17} strokeWidth={1.6} />
                <span>{label}</span>
              </Link>
            </div>
          ))}
        </nav>
        <div className="arc-sidebar-footer">
          <span>{S.app.name}</span>
          <span>v{health.data?.version ?? "0.1"}</span>
        </div>
      </aside>
      <div className="arc-main-shell">
        <header className="arc-topbar">
          <span className="arc-breadcrumb">
            {kb?.name}
            <span>/</span>
            <strong>{tabLabel ?? S.arcadia.home}</strong>
          </span>
          <HeaderActions
            link={{ to: "/docs", label: S.nav.docs }}
            user={me.data}
          />
        </header>
        <main className="arc-main">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
