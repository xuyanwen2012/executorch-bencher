import { GlobalRegistrator } from "@happy-dom/global-registrator";

// A real origin rather than about:blank: the API client issues relative
// requests (`/api/v1/...`), and happy-dom refuses to construct a Request
// from a relative URL on about:blank. Component tests answer those requests
// through openapi-fetch middleware, so nothing reaches this host.
GlobalRegistrator.register({ url: "http://dashboard.test/" });
