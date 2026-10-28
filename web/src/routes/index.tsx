import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { healthQuery } from "@/api/health";
import { ServerStatus } from "@/components/server-status";

export const Route = createFileRoute("/")({ component: Dashboard });

function Dashboard() {
  const health = useQuery(healthQuery);
  return <ServerStatus query={health} onRetry={() => void health.refetch()} />;
}
