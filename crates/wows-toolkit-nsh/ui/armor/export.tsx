import type { JSX } from "nsh";
import { Scroll, Text, View } from "nsh";
import { Dialog } from "nsh/base";
import { Armor } from "wows-toolkit/native";
import { Button, Surface } from "../components";
import { t } from "../i18n";
import { colors } from "../theme";
import { runExport } from "./dock";
import { armor, type CamoView, call, type ExportDialogView } from "./state";
import { CheckRow, ChoiceRow, Hint, ToolButton } from "./widgets";

/** A 96 px label column with its control beside it. */
function Row(props: { label: string; hint?: string; children: JSX.Element }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
      <Text style={{ width: 96, flexShrink: 0, paddingTop: 4, fontSize: 12, color: colors.popoverForeground }}>{props.label}</Text>
      <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
        {props.children}
        <If when={props.hint}>
          <Hint text={props.hint ?? ""}/>
        </If>
      </View>
    </View>
  );
}

function Camos(props: { schemes: CamoView[]; disabled: boolean }) {
  return (
    <Each of={props.schemes} key={(scheme) => scheme.id}>
      {(scheme) => <CheckRow id={`armor-export-camo-${scheme().id}`} label={scheme().name} checked={scheme().selected} disabled={props.disabled} onChange={(on) => call(Armor.setExportCamo(scheme().id, on))}/>}
    </Each>
  );
}

function SizeLine(props: { dialog: ExportDialogView }) {
  const size = () => props.dialog.size;
  return (
    <View style={{ gap: 2 }}>
      <If when={size().kind === "waiting"}>
        <Hint text={t("ui.armor.export.loading_details")}/>
      </If>
      <If when={size().estimate}>
        <Text style={{ fontSize: 12.5, color: colors.popoverForeground }}>{size().estimate}</Text>
      </If>
      <If when={size().breakdown}>
        <Hint text={size().breakdown ?? ""}/>
      </If>
      <If when={size().incomplete}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Hint tone="warn" text={size().incomplete ?? ""}/>
          <ToolButton id="armor-export-retry" label={t("ui.buttons.retry")} onPress={() => call(Armor.retryExportPricing())}/>
        </View>
      </If>
    </View>
  );
}

/** The GLB export options for a ship chosen in the ships list. */
export function ExportDialog() {
  const dialog = () => armor()?.exportDialog ?? null;
  return (
    <Dialog open={dialog() !== null} onOpenChange={({ open }) => { if (!open) call(Armor.cancelExportDialog()); }}>
      <If when={dialog()}>
        {(current) => {
          const mesh = () => current().contents !== "armor";
          return (
            <Surface style={{ width: 460, maxHeight: "90%", padding: 16, gap: 12, backgroundColor: colors.popover }}>
              <Text style={{ fontSize: 15, fontWeight: "600", color: colors.popoverForeground }}>{`${t("ui.armor.export_model")} - ${current().ship}`}</Text>
              <Scroll style={{ flexShrink: 1 }}>
                <View style={{ gap: 12 }}>
                  <Row label={t("ui.armor.export.contents")}>
                    <ChoiceRow
                      choices={[
                        { value: "mesh", label: t("ui.armor.export.contents_mesh"), selected: current().contents === "mesh", id: "armor-export-contents-mesh" },
                        { value: "armor", label: t("ui.armor.export.contents_armor"), selected: current().contents === "armor", id: "armor-export-contents-armor" },
                        { value: "meshAndArmor", label: t("ui.armor.export.contents_both"), selected: current().contents === "meshAndArmor", id: "armor-export-contents-both" },
                      ]}
                      onSelect={(contents) => call(Armor.setExportContents(contents))}
                    />
                  </Row>
                  <If when={current().hulls.length > 0}>
                    <Row label={t("ui.armor.export.hull")}>
                      <ChoiceRow
                        disabled={current().hullsDisabled}
                        choices={current().hulls.map((hull) => ({ value: hull.key, label: hull.label, selected: hull.selected, id: `armor-export-hull-${hull.key}` }))}
                        onSelect={(key) => call(Armor.setExportHull(key))}
                      />
                    </Row>
                  </If>
                  <If when={current().lodCount > 1}>
                    <Row label={t("ui.armor.export.lod")} hint={t("ui.armor.export.lod_tooltip")}>
                      <ChoiceRow
                        disabled={!mesh()}
                        choices={Array.from({ length: current().lodCount }, (_, lod) => ({ value: lod, label: String(lod), selected: current().lod === lod, id: `armor-export-lod-${lod}` }))}
                        onSelect={(lod) => call(Armor.setExportLod(lod))}
                      />
                    </Row>
                  </If>
                  <Row label={t("ui.armor.export.res")}>
                    <ChoiceRow
                      disabled={!mesh()}
                      choices={[
                        { value: "full", label: t("ui.armor.export.res_full"), selected: current().resolution === "full", id: "armor-export-res-full" },
                        { value: "px2048", label: t("ui.armor.export.res_2048"), selected: current().resolution === "px2048", id: "armor-export-res-2048" },
                        { value: "px1024", label: t("ui.armor.export.res_1024"), selected: current().resolution === "px1024", id: "armor-export-res-1024" },
                        { value: "px512", label: t("ui.armor.export.res_512"), selected: current().resolution === "px512", id: "armor-export-res-512" },
                      ]}
                      onSelect={(resolution) => call(Armor.setExportResolution(resolution))}
                    />
                  </Row>
                  <If when={current().camos.own.length > 0 || current().camos.groups.length > 0}>
                  <Row label={t("ui.armor.export.camos")}>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                      <ToolButton id="armor-export-camos-none" label={t("ui.armor.export.camo_select_none")} disabled={!mesh()} onPress={() => call(Armor.exportCamosNone())}/>
                      <ToolButton id="armor-export-camos-reset" label={t("ui.armor.export.camo_reset")} disabled={!mesh()} onPress={() => call(Armor.exportCamosReset())}/>
                    </View>
                    <Scroll style={{ maxHeight: 240 }}>
                      <View style={{ gap: 3 }}>
                        <Text testID="armor-export-camo-stock" style={{ fontSize: 12, opacity: mesh() ? 1 : 0.5, color: current().camos.noneTicked ? colors.popoverForeground : colors.mutedForeground }}>{t("ui.armor.camo_none")}</Text>
                        <Camos schemes={current().camos.own} disabled={!mesh()}/>
                        <Each of={current().camos.groups} key={(group) => group.heading}>
                          {(group) => (
                            <View style={{ gap: 3 }}>
                              <Text style={{ fontSize: 12, opacity: mesh() ? 1 : 0.5, color: colors.mutedForeground }}>{t(group().heading)}</Text>
                              <Camos schemes={group().schemes} disabled={!mesh()}/>
                            </View>
                          )}
                        </Each>
                      </View>
                    </Scroll>
                  </Row>
                  </If>
                  <If when={current().error}>
                    <Hint tone="error" text={current().error ?? ""}/>
                  </If>
                  <SizeLine dialog={current()}/>
                  <Hint text={t("ui.armor.export_disclaimer")}/>
                </View>
              </Scroll>
              <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
                <View testID="armor-export-cancel"><Button label={t("ui.buttons.cancel")} onPress={() => call(Armor.cancelExportDialog())}/></View>
                <View testID="armor-export-confirm">
                  <Button
                    label={t("ui.armor.export_button")}
                    variant="primary"
                    disabled={!current().ready}
                    onPress={() => void runExport(() => Armor.confirmExportDialog())}
                  />
                </View>
              </View>
            </Surface>
          );
        }}
      </If>
    </Dialog>
  );
}
