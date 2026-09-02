import { FieldGroup, Hash } from "../../components/FieldGroup";
import { Badge } from "../../components/State";
import type { Run } from "./shared";

export function ModelGroup({ run }: { run: Run }) {
  return (
    <FieldGroup
      title="Model"
      note={run.model_asset?.available ? undefined : "file missing on disk"}
      fields={
        run.model_asset
          ? [
              { label: "Name", value: run.model_asset.original_name, mono: true },
              { label: "SHA-256", value: <Hash value={run.model_asset.sha256} full /> },
              {
                label: "Availability",
                value: run.model_asset.available ? (
                  <Badge tone="ok" plain>
                    available
                  </Badge>
                ) : (
                  <Badge tone="danger">unavailable</Badge>
                ),
              },
              { label: "Asset ID", value: run.model_asset.id, mono: true },
            ]
          : [{ label: "Model", value: null }]
      }
    />
  );
}
