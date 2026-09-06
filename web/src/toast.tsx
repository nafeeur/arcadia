// Forwarding shim for the old path: toast now lives in ui/ (it's a component). Pages still import "./toast"
export { toast, ToastHost } from "./ui/toast";
