import type { JSX } from "nsh";
import { effect, Scroll, signal, Text, View } from "nsh";
import { Checkbox, Input, Progress } from "nsh/base";
import { LauncherSession, type AccountState, type DownloadRow, type GameRow, type InstallRow, type LauncherState, type StatusLine } from "wows-toolkit/native";
import { Button, Surface } from "./components";
import { t } from "./i18n";
import { applyWowsDir } from "./settings";
import { appSettings } from "./settings_store";
import * as toast from "./shell/toast";
import { colors, controlRadius, semantic } from "./theme";

const session = new LauncherSession();
const realms = LauncherSession.realms();
const state = signal<LauncherState | null>(null);
const safeMode = signal(false);
const realm = signal(0);
const showSignIn = signal(false);
const useBackupCode = signal(false);
const username = signal("");
const password = signal("");
const code = signal("");
const extraArgs = signal<string | null>(null);
const envVars = signal<string | null>(null);
let running = false;
let gamesRequested = false;

function start() {
  if (running) return;
  running = true;
  void session.run((next) => adopt(next)).catch((error) => console.error(`launcher: ${errorMessage(error)}`));
}

function adopt(next: LauncherState) {
  // The text fields hold their own edits; the first state fills them.
  if (next.loaded && extraArgs() === null) extraArgs.set(next.options.extraArgs);
  if (next.loaded && envVars() === null) envVars.set(next.options.envVars);
  state.set(next);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// Background work starts with the app, as discovery and the saved session do
// in the other front ends; the catalog of games is checked when the tab shows.
start();

function toneColor(tone: string): string {
  switch (tone) {
    case "dim": return colors.mutedForeground;
    case "success": return semantic.ok;
    case "warning": return semantic.warn;
    case "danger": return colors.destructive;
    default: return colors.cardForeground;
  }
}

function Line(props: { line: StatusLine }) {
  return (
    <View style={{ gap: 2 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <If when={props.line.busy}>
          <View style={{ width: 60 }}><Progress value={0} indeterminate accessibilityLabel={props.line.text}/></View>
        </If>
        <Text style={{ fontSize: 12.5, color: toneColor(props.line.tone) }}>{props.line.text}</Text>
      </View>
      <If when={props.line.detail}>
        <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{props.line.detail}</Text>
      </If>
    </View>
  );
}

function Section(props: { title: string; children: JSX.Element }) {
  return (
    <View style={{ gap: 8 }}>
      <Text style={{ fontSize: 13, fontWeight: "600", color: colors.foreground }}>{props.title}</Text>
      <Surface style={{ padding: 12, gap: 10 }}>{props.children}</Surface>
    </View>
  );
}

function Card(props: { children: JSX.Element }) {
  return <Surface style={{ padding: 12, gap: 8, backgroundColor: colors.background }}>{props.children}</Surface>;
}

function Toggle(props: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <Checkbox
      accessibilityLabel={props.label}
      checked={props.checked}
      class="group flex-row items-center gap-2"
      indicatorClass="w-[16px] h-[16px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
      indicator={<Text class="text-[10px] text-primary-foreground">{props.checked ? "x" : ""}</Text>}
      onChange={({ checked }) => props.onChange(checked)}
    >
      <Text style={{ fontSize: 12.5, color: colors.cardForeground }}>{props.label}</Text>
    </Checkbox>
  );
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

function Field(props: { label: string; value: string; masked?: boolean; onChange: (value: string) => void; onSubmit?: () => void }) {
  return (
    <Input
      accessibilityLabel={props.label}
      placeholder={props.label}
      value={props.value}
      masked={props.masked}
      onChange={({ value }) => props.onChange(value)}
      onSubmit={() => props.onSubmit?.()}
      style={[fieldStyle, { width: 240 }]}
    />
  );
}

async function act(work: () => Promise<LauncherState> | LauncherState) {
  try {
    adopt(await work());
  } catch (error) {
    toast.failed(errorMessage(error));
  }
}

function autodetectProton() {
  const outcome = session.autodetectProton();
  adopt(outcome.state);
  if (outcome.level === "success") toast.ok(outcome.message);
  else if (outcome.level === "warning") toast.warn(outcome.message);
  else toast.failed(outcome.message);
}

function signIn() {
  if (state()?.account.inProgress) return;
  // The password leaves the field the moment it is used.
  const secret = password();
  password.set("");
  void act(() => session.signIn(realm(), username(), secret));
}

function submitCode() {
  if (state()?.account.inProgress) return;
  const entered = code();
  code.set("");
  adopt(session.submitCode(entered, useBackupCode()));
}

function Spinner(props: { label: string }) {
  return <View style={{ width: 60 }}><Progress value={0} indeterminate accessibilityLabel={props.label}/></View>;
}

function ErrorLine(props: { text: string | null | undefined }) {
  return (
    <If when={props.text}>
      <Text style={{ fontSize: 12.5, color: colors.destructive }}>{props.text ?? ""}</Text>
    </If>
  );
}

function SignedIn(props: { account: AccountState }) {
  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{props.account.nickname ?? ""}</Text>
        <Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{props.account.region ?? ""}</Text>
      </View>
      <If when={props.account.expiry}>{(expiry) => <Line line={expiry()}/>}</If>
      <If when={props.account.notPersisted}>
        <Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{t("ui.launcher.account.not_persisted")}</Text>
      </If>
      <View style={{ flexDirection: "row" }}>
        <Button label={t("ui.launcher.account.sign_out")} onPress={() => adopt(session.signOut())}/>
      </View>
    </View>
  );
}

function TwoFactor(props: { account: AccountState }) {
  return (
    <View style={{ gap: 8 }}>
      <Text style={{ fontSize: 13, color: colors.cardForeground }}>{t("ui.launcher.account.code_required")}</Text>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <Field label={t("ui.launcher.account.code")} value={code()} onChange={(value) => code.set(value)} onSubmit={submitCode}/>
        <Toggle label={t("ui.launcher.account.use_backup_code")} checked={useBackupCode()} onChange={(checked) => useBackupCode.set(checked)}/>
      </View>
      <ErrorLine text={props.account.error}/>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <Button label={t("ui.launcher.account.submit_code")} disabled={props.account.inProgress} onPress={submitCode}/>
        <If when={props.account.inProgress}><Spinner label={t("ui.launcher.account.submit_code")}/></If>
      </View>
    </View>
  );
}

function Captcha() {
  return (
    <View style={{ gap: 8 }}>
      <Text style={{ fontSize: 12.5, color: toneColor("warning") }}>{t("ui.launcher.account.captcha")}</Text>
      <View style={{ flexDirection: "row" }}>
        <Button
          label={t("ui.launcher.retry")}
          onPress={() => {
            adopt(session.dismissCaptcha());
            showSignIn.set(true);
          }}
        />
      </View>
    </View>
  );
}

function SignInPrompt() {
  return (
    <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
      <Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{t("ui.launcher.account.signed_out")}</Text>
      <Button label={t("ui.launcher.account.sign_in")} onPress={() => showSignIn.set(true)}/>
    </View>
  );
}

function SignInForm(props: { account: AccountState }) {
  return (
    <View style={{ gap: 8 }}>
      <View role="radiogroup" accessibilityLabel={t("ui.launcher.account.heading")} style={{ flexDirection: "row", gap: 4 }}>
        <Each of={realms}>
          {(label, index) => (
            <Button label={label()} variant={realm() === index() ? "primary" : "secondary"} onPress={() => realm.set(index())}/>
          )}
        </Each>
      </View>
      <View style={{ flexDirection: "row", gap: 8 }}>
        <Field label={t("ui.launcher.account.username")} value={username()} onChange={(value) => username.set(value)}/>
        <Field
          label={t("ui.launcher.account.password")}
          value={password()}
          masked
          onChange={(value) => password.set(value)}
          onSubmit={() => signIn()}
        />
      </View>
      <ErrorLine text={props.account.error}/>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <Button
          label={t("ui.launcher.account.sign_in")}
          variant="primary"
          disabled={props.account.inProgress}
          onPress={() => signIn()}
        />
        <If when={props.account.inProgress}><Spinner label={t("ui.launcher.account.sign_in")}/></If>
      </View>
    </View>
  );
}

function Account(props: { account: AccountState }) {
  const kind = () => props.account.kind;
  return (
    <Section title={t("ui.launcher.account.heading")}>
      <If when={kind() === "signedIn"} else={
        <If when={kind() === "twoFactor"} else={
          <If when={kind() === "captcha"} else={
            <If when={showSignIn()} else={<SignInPrompt/>}>
              <SignInForm account={props.account}/>
            </If>
          }>
            <Captcha/>
          </If>
        }>
          <TwoFactor account={props.account}/>
        </If>
      }>
        <SignedIn account={props.account}/>
      </If>
    </Section>
  );
}

function RunningDownload(props: { row: DownloadRow }) {
  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <If when={props.row.fraction !== null && props.row.fraction !== undefined} else={<Spinner label={props.row.status.text}/>}>
          <View style={{ width: 220 }}>
            <Progress value={(props.row.fraction ?? 0) * 100} accessibilityLabel={props.row.status.text}/>
          </View>
          <Text style={{ fontSize: 12.5, color: colors.cardForeground }}>{props.row.sizes ?? ""}</Text>
        </If>
        <Button label={t("ui.launcher.pause")} variant="ghost" onPress={() => adopt(session.pause(props.row.key))}/>
        <Button label={t("ui.launcher.cancel")} variant="ghost" onPress={() => adopt(session.cancel(props.row.key))}/>
      </View>
      <Text numberOfLines={1} style={{ fontSize: 12.5, color: colors.cardForeground }}>{props.row.status.text}</Text>
    </View>
  );
}

function Download(props: { row: DownloadRow }) {
  return (
    <If when={props.row.phase === "running"} else={
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <Line line={props.row.status}/>
        <If when={props.row.phase === "paused"}>
          <Button label={t("ui.launcher.resume")} onPress={() => void act(() => session.resume(props.row.key))}/>
        </If>
      </View>
    }>
      <RunningDownload row={props.row}/>
    </If>
  );
}

function Install(props: { row: InstallRow }) {
  const launch = () => {
    try {
      session.launch(props.row.key, safeMode());
    } catch (error) {
      toast.failed(errorMessage(error));
    }
  };
  return (
    <Card>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
        <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{props.row.source}</Text>
        <Text style={{ fontSize: 12.5, fontFamily: "monospace", color: colors.cardForeground }}>{props.row.path}</Text>
        <If when={props.row.version}>
          <Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{props.row.version ?? ""}</Text>
        </If>
      </View>
      <If when={!props.row.managedBySteam}>
        <View style={{ gap: 8 }}>
          <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
            <If when={props.row.catalog}>{(catalog) => <Line line={catalog()}/>}</If>
            <Button label={t("ui.launcher.recheck")} variant="ghost" onPress={() => adopt(session.recheck(props.row.key))}/>
          </View>
          <If when={props.row.download}>{(download) => <Download row={download()}/>}</If>
          <If when={props.row.canDownload}>
            <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
              <Button label={t("ui.launcher.download")} onPress={() => void act(() => session.download(props.row.key))}/>
              <Text style={{ flex: 1, fontSize: 12, color: colors.mutedForeground }}>{t("ui.launcher.download_hint")}</Text>
            </View>
          </If>
        </View>
      </If>
      <If when={props.row.needsProton}>
        <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
          <Text style={{ fontSize: 12.5, color: toneColor("warning") }}>{t("ui.launcher.proton.needed")}</Text>
          <Button label={t("ui.launcher.proton.set_up")} onPress={() => autodetectProton()}/>
        </View>
      </If>
      <View style={{ flexDirection: "row", gap: 8 }}>
        <Button label={t("ui.launcher.launch")} variant="primary" onPress={launch}/>
        <Button label={t("ui.launcher.use_for_toolkit")} onPress={() => applyWowsDir(props.row.path)}/>
      </View>
    </Card>
  );
}

function Installs(props: { current: LauncherState }) {
  return (
    <Section title={t("ui.launcher.installs_heading")}>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <Button label={t("ui.launcher.rescan")} onPress={() => adopt(session.rescan())}/>
        <Button label={t("ui.launcher.add_install")} onPress={() => void act(() => session.addInstall())}/>
        <If when={props.current.discovering}><Spinner label={t("ui.launcher.rescan")}/></If>
        <View style={{ flex: 1 }}/>
        <Toggle label={t("ui.launcher.safe_mode")} checked={safeMode()} onChange={(checked) => safeMode.set(checked)}/>
      </View>
      <If when={props.current.installs.length === 0 && !props.current.discovering}>
        <Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{t("ui.launcher.no_installs")}</Text>
      </If>
      <Each of={props.current.installs} key={(row) => row.key}>
        {(row) => <Install row={row()}/>}
      </Each>
    </Section>
  );
}

function PathRow(props: { label: string; path: string | null | undefined; onBrowse: () => void }) {
  return (
    <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
      <Text style={{ width: 60, fontSize: 13, color: colors.cardForeground }}>{props.label}</Text>
      <Text style={{ flex: 1, fontSize: 12.5, color: props.path ? colors.cardForeground : colors.mutedForeground }}>
        {props.path ?? t("ui.launcher.not_set")}
      </Text>
      <Button label={t("ui.launcher.browse")} onPress={props.onBrowse}/>
    </View>
  );
}

function OptionsText(props: { label: string; value: string; onChange: (value: string) => void; onCommit: () => void }) {
  return (
    <View style={{ gap: 6 }}>
      <Text style={{ fontSize: 13, color: colors.cardForeground }}>{props.label}</Text>
      <Input
        accessibilityLabel={props.label}
        multiline
        value={props.value}
        onChange={({ value }) => props.onChange(value)}
        onBlur={props.onCommit}
        style={[fieldStyle, { minHeight: 52, paddingVertical: 6 }]}
      />
    </View>
  );
}

function Options(props: { current: LauncherState }) {
  return (
    <Section title={t("ui.launcher.options.heading")}>
      <OptionsText
        label={t("ui.launcher.options.extra_args")}
        value={extraArgs() ?? ""}
        onChange={(value) => extraArgs.set(value)}
        onCommit={() => adopt(session.setExtraArgs(extraArgs() ?? ""))}
      />
      <OptionsText
        label={t("ui.launcher.options.env_vars")}
        value={envVars() ?? ""}
        onChange={(value) => envVars.set(value)}
        onCommit={() => adopt(session.setEnvVars(envVars() ?? ""))}
      />
      <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{t("ui.launcher.proton.heading")}</Text>
      <Text style={{ fontSize: 12.5, lineHeight: 18, color: colors.mutedForeground }}>{t("ui.launcher.proton.description")}</Text>
      <PathRow
        label={t("ui.launcher.proton.binary")}
        path={props.current.options.protonBinary}
        onBrowse={() => void act(() => session.browseProtonBinary())}
      />
      <PathRow
        label={t("ui.launcher.proton.prefix")}
        path={props.current.options.protonPrefix}
        onBrowse={() => void act(() => session.browseProtonPrefix())}
      />
      <View style={{ flexDirection: "row" }}>
        <Button label={t("ui.launcher.proton.detect")} onPress={() => autodetectProton()}/>
      </View>
    </Section>
  );
}

function Game(props: { row: GameRow }) {
  return (
    <Card>
      <View style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
        <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{props.row.name}</Text>
        <If when={props.row.version}>{(version) => <Line line={version()}/>}</If>
        <If when={props.row.retry}>
          <Button label={t("ui.launcher.retry")} variant="ghost" onPress={() => adopt(session.checkGames())}/>
        </If>
      </View>
      <If when={props.row.download}>{(download) => <Download row={download()}/>}</If>
      <View style={{ flexDirection: "row" }}>
        <Button
          label={t("ui.launcher.install")}
          disabled={!props.row.canInstall}
          onPress={() => void act(() => session.installGame(props.row.index))}
        />
      </View>
    </Card>
  );
}

function InstallGame(props: { current: LauncherState }) {
  return (
    <Section title={t("ui.launcher.install_heading")}>
      <Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{t("ui.launcher.install_hint")}</Text>
      <Each of={props.current.games} key={(row) => row.index}>
        {(row) => <Game row={row()}/>}
      </Each>
    </Section>
  );
}

/** The Launcher tab. */
export function LauncherView(props: { visible: boolean }) {
  // The launcher's own requests go through the app's proxy setting.
  effect(() => {
    const proxy = appSettings()?.proxyUrl;
    if (proxy !== undefined) session.setProxy(proxy);
  });
  // The catalog of games is checked once the tab first shows with its
  // settings read; Retry asks again for a check that failed.
  effect(() => {
    if (gamesRequested || !props.visible || !state()?.loaded) return;
    gamesRequested = true;
    adopt(session.checkGames());
  });
  return (
    <If when={state()?.loaded ? state() : null} else={
      <View style={{ padding: 16 }}>
        <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{t("ui.launcher.loading")}</Text>
      </View>
    }>
      {(current) => (
        <Scroll style={{ flex: 1 }}>
          <View style={{ padding: 16, gap: 24 }}>
            <Account account={current().account}/>
            <Installs current={current()}/>
            <Options current={current()}/>
            <InstallGame current={current()}/>
          </View>
        </Scroll>
      )}
    </If>
  );
}
