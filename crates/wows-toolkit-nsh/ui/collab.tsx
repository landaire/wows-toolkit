import { Clipboard, effect, signal, Text, View } from "nsh";
import { Checkbox, Input, Popover, Progress } from "nsh/base";
import { Collab, type CollabState, type RosterEntry, type SharedWindowEntry } from "wows-toolkit/native";
import { Button, Surface } from "./components";
import { t } from "./i18n";
import { CheckMark } from "./icons";
import { appSettings, editSetting } from "./settings_store";
import { beforeRevealingAddress } from "./shell/notices";
import * as toast from "./shell/toast";
import { colors, controlRadius, semantic } from "./theme";

const state = signal<CollabState>(Collab.state());
const open = signal(false);
const token = signal("");

Collab.on(({ state: next, notices }) => {
  state.set(next);
  for (const notice of notices) {
    if (notice.level === "error") toast.failed(notice.text);
    else if (notice.level === "warning") toast.warn(notice.text);
    else toast.info(notice.text);
  }
});

// The name is a shared setting; the Settings tab edits it too.
effect(() => {
  const saved = appSettings()?.collabDisplayName;
  if (saved !== undefined && saved !== Collab.state().displayName) Collab.setDisplayName(saved);
});

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function Rule() {
  return <View style={{ height: 1, marginVertical: 2, backgroundColor: colors.border }}/>;
}

function Heading(props: { text: string; color?: string }) {
  return <Text style={{ fontSize: 13, fontWeight: "600", color: props.color ?? colors.popoverForeground }}>{props.text}</Text>;
}

function Dim(props: { text: string }) {
  return <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{props.text}</Text>;
}

const fieldStyle = {
  minHeight: 30,
  paddingHorizontal: 9,
  borderWidth: 1,
  borderColor: colors.input,
  borderRadius: controlRadius(),
  color: colors.foreground,
  backgroundColor: colors.background,
  fontSize: 13,
};

function Toggle(props: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <Checkbox
      accessibilityLabel={props.label}
      checked={props.checked}
      class="group flex-row items-center gap-2"
      indicatorClass="w-[16px] h-[16px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
      indicator={<CheckMark checked={props.checked}/>}
      onChange={({ checked }) => props.onChange(checked)}
    >
      <Text style={{ fontSize: 12.5, color: colors.popoverForeground }}>{props.label}</Text>
    </Checkbox>
  );
}

function start(work: () => void) {
  // Hosting or joining tells every peer where this machine is, which the
  // reader is asked about first.
  beforeRevealingAddress(() => {
    try {
      work();
    } catch (error) {
      toast.failed(errorMessage(error));
    }
  });
}

/** A button that copies `value` and says `said` once it has. */
function CopyButton(props: { label: string; value: string; said: string }) {
  return (
    <Button
      label={props.label}
      onPress={() => {
        void Clipboard.writeText(props.value)
          .then(() => toast.info(props.said))
          .catch((error) => toast.failed(errorMessage(error)));
      }}
    />
  );
}

function Start(props: { current: CollabState }) {
  const named = () => props.current.displayName.trim().length > 0;
  return (
    <View style={{ gap: 8 }}>
      <Heading text={t("ui.collab.display_name")}/>
      <Input
        accessibilityLabel={t("ui.collab.display_name")}
        placeholder={t("ui.collab.display_name_hint")}
        value={props.current.displayName}
        onChange={({ value }) => Collab.setDisplayName(value)}
        onBlur={() => void editSetting("collabDisplayName", Collab.state().displayName)}
        style={fieldStyle}
      />
      <If when={!named()}>
        <Dim text={t("ui.collab.enter_display_name")}/>
      </If>
      <Rule/>
      <Heading text={t("ui.collab.host_session")}/>
      <Button
        label={t("ui.collab.start_session")}
        disabled={!named()}
        onPress={() => start(() => Collab.host())}
      />
      <Rule/>
      <Heading text={t("ui.collab.join_session")}/>
      <Input
        accessibilityLabel={t("ui.collab.session_token")}
        placeholder="toolkit-..."
        value={token()}
        onChange={({ value }) => token.set(value)}
        style={fieldStyle}
      />
      <Button
        label={t("ui.collab.paste_and_join")}
        disabled={!named()}
        onPress={() => {
          const pasted = token();
          start(() => Collab.join(pasted));
        }}
      />
      <If when={props.current.failure}>
        <Text style={{ fontSize: 12, color: semantic.error }}>{props.current.failure ?? ""}</Text>
      </If>
    </View>
  );
}

function Waiting() {
  return (
    <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
      <View style={{ width: 48 }}><Progress value={0} indeterminate accessibilityLabel={t("ui.collab.starting")}/></View>
      <Text style={{ flex: 1, fontSize: 13, color: colors.popoverForeground }}>{t("ui.collab.starting")}</Text>
      <Button label={t("ui.buttons.cancel")} onPress={() => Collab.leave()}/>
    </View>
  );
}

function Failed(props: { reason: string }) {
  return (
    <View style={{ gap: 8 }}>
      <Text style={{ fontSize: 13, color: semantic.error }}>{props.reason}</Text>
      <View style={{ flexDirection: "row" }}>
        <Button label={t("ui.collab.leave")} onPress={() => Collab.leave()}/>
      </View>
    </View>
  );
}

function TokenSection(props: { current: CollabState; token: string }) {
  return (
    <View style={{ gap: 6 }}>
      <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{t("ui.collab.session_token")}</Text>
      <View style={{ flexDirection: "row", gap: 6, alignItems: "center" }}>
        <Text style={{ flex: 1, fontSize: 12, fontFamily: "monospace", color: colors.popoverForeground }}>{props.current.tokenShown ?? ""}</Text>
        <Button
          label={t("ui.collab.toggle_visibility")}
          variant="ghost"
          onPress={() => Collab.toggleToken()}
        />
      </View>
      <View style={{ flexDirection: "row", gap: 6, flexWrap: "wrap" }}>
        <CopyButton label={t("ui.collab.copy_token")} value={props.token} said={t("ui.collab.token_copied")}/>
        <If when={props.current.webLink}>
          {(link) => <CopyButton label={t("ui.collab.copy_web_link")} value={link()} said={t("ui.collab.web_link_copied")}/>}
        </If>
        <If when={props.current.localhostLink}>
          {(link) => <CopyButton label={t("ui.collab.copy_localhost_link")} value={link()} said={t("ui.collab.localhost_copied")}/>}
        </If>
      </View>
    </View>
  );
}

function SharedWindow(props: {
  window: SharedWindowEntry;
  maySteer: boolean;
  onWatch?: (window: SharedWindowEntry) => void;
}) {
  return (
    <View accessibilityLabel={props.window.replayName} style={{ flexDirection: "row", gap: 6, alignItems: "center" }}>
      <Text numberOfLines={1} style={{ flex: 1, fontSize: 12, color: colors.popoverForeground }}>{props.window.replayName}</Text>
      <Dim text={props.window.map}/>
      <If when={props.onWatch}>
        {(watch) => <Button label={t("ui.collab.open")} onPress={() => watch()(props.window)}/>}
      </If>
      <If when={props.maySteer}>
        <Button
          label={t("ui.collab.open_for_everyone")}
          onPress={() => {
            try {
              Collab.openForEveryone(props.window.replayId);
            } catch (error) {
              toast.failed(errorMessage(error));
            }
          }}
        />
      </If>
    </View>
  );
}

function Participant(props: { peer: RosterEntry }) {
  return (
    <View style={{ flexDirection: "row", gap: 6, alignItems: "center" }}>
      <Text style={{ flex: 1, fontSize: 12, color: colors.popoverForeground }}>{props.peer.name}</Text>
      <If when={props.peer.isMe}><Dim text={t("ui.collab.you")}/></If>
      <If when={props.peer.role}>{(role) => <Dim text={role()}/>}</If>
      <If when={props.peer.promotable}>
        <Button
          label={t("ui.collab.promote_cohost")}
          onPress={() => {
            try {
              Collab.promote(props.peer.userId);
            } catch (error) {
              toast.failed(errorMessage(error));
            }
          }}
        />
      </If>
    </View>
  );
}

function Permissions(props: { current: CollabState }) {
  return (
    <View style={{ gap: 6 }}>
      <Heading text={t("ui.collab.permissions")}/>
      <Toggle
        label={t("ui.collab.lock_annotations")}
        checked={props.current.annotationsLocked}
        onChange={(checked) => Collab.setPermissions(checked, props.current.settingsLocked)}
      />
      <Toggle
        label={t("ui.collab.lock_settings")}
        checked={props.current.settingsLocked}
        onChange={(checked) => Collab.setPermissions(props.current.annotationsLocked, checked)}
      />
      <View style={{ flexDirection: "row" }}>
        <View tooltip={t("ui.collab.reset_overrides_tooltip")}>
          <Button label={t("ui.collab.reset_overrides")} onPress={() => Collab.resetOverrides()}/>
        </View>
      </View>
    </View>
  );
}

function Active(props: {
  current: CollabState;
  onOpenTacticsBoard: () => void;
  onWatchSharedWindow?: (window: SharedWindowEntry) => void;
}) {
  return (
    <View style={{ gap: 8 }}>
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
        <Heading text={props.current.heading} color={semantic.error}/>
        <Button label={t("ui.collab.leave")} onPress={() => Collab.leave()}/>
      </View>
      <Dim text={props.current.connectedCount}/>
      <If when={props.current.token}>
        {(held) => (
          <View style={{ gap: 8 }}>
            <Rule/>
            <TokenSection current={props.current} token={held()}/>
          </View>
        )}
      </If>
      <Rule/>
      <Button
        label={t("ui.collab.tactics_board")}
        onPress={() => {
          open.set(false);
          props.onOpenTacticsBoard();
        }}
      />
      <If when={props.current.sharedWindows.length > 0}>
        <View style={{ gap: 6 }}>
          <Rule/>
          <Heading text={t("ui.collab.shared_windows")}/>
          <Each of={props.current.sharedWindows} key={(window) => window.replayId}>
            {(window) => (
              <SharedWindow window={window()} maySteer={props.current.maySteer} onWatch={props.onWatchSharedWindow}/>
            )}
          </Each>
        </View>
      </If>
      <Rule/>
      <Each of={props.current.roster} key={(peer) => peer.userId}>
        {(peer) => <Participant peer={peer()}/>}
      </Each>
      <If when={props.current.maySteer}>
        <View style={{ gap: 6 }}>
          <Rule/>
          <Permissions current={props.current}/>
        </View>
      </If>
    </View>
  );
}

/**
 * The session button and the popover under it: host or join a session,
 * then steer it. A running session colors the button, since it is what the
 * reader most needs to know is running.
 */
export function SessionButton(props: {
  onOpenTacticsBoard: () => void;
  onWatchSharedWindow?: (window: SharedWindowEntry) => void;
}) {
  return (
    <Popover
      open={open()}
      onOpenChange={({ open: next }) => {
        open.set(next);
      }}
      placement="bottom"
      align="end"
      offset={5}
      margin={8}
      trigger={
        <View
          role="button"
          accessibilityLabel={t("ui.collab.session")}
          hoverStyle={{ opacity: 0.86 }}
          style={{
            height: 30,
            paddingHorizontal: 10,
            borderRadius: controlRadius(),
            justifyContent: "center",
            backgroundColor: state().active ? colors.destructive : "transparent",
          }}
        >
          <Text style={{ fontSize: 12.5, color: state().active ? colors.primaryForeground : colors.mutedForeground }}>
            {t("ui.collab.session")}
          </Text>
        </View>
      }
    >
      <Surface style={{ width: 320, padding: 10, backgroundColor: colors.popover }}>
        <If when={state().status === "idle"} else={
          <If when={state().status === "starting"} else={
            <If when={state().status === "error"} else={
              <Active
                current={state()}
               
                onOpenTacticsBoard={props.onOpenTacticsBoard}
                onWatchSharedWindow={props.onWatchSharedWindow}
              />
            }>
              <Failed reason={state().error ?? ""}/>
            </If>
          }>
            <Waiting/>
          </If>
        }>
          <Start current={state()}/>
        </If>
      </Surface>
    </Popover>
  );
}
