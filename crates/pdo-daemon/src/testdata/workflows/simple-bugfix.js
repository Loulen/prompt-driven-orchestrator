export const meta = {
  name: 'simple-bugfix',
  description: "Port du pipeline PDO simple-bugfix : triage d'un bug, boucle fix<->test bornee, puis commit",
  phases: [
    { title: 'Triage',   detail: 'Debugger reproduit et tranche bug vs pas-bug' },
    { title: 'Fix loop', detail: 'Implementer corrige, Tester verifie ; jusqu a 5 tours' },
    { title: 'Ship',     detail: 'Commit du correctif sur la branche courante' },
  ],
}

const bugReport = typeof args === 'string' ? args : (args && args.bug) || ''
if (!bugReport) log('Aucun bug passe en args.')

const TRIAGE_SCHEMA = {
  type: 'object', required: ['verdict', 'how_to_reproduce'],
  properties: {
    verdict: { type: 'string', enum: ['Bug', 'Feature', 'Unsure', 'Not Reproduced'] },
    how_to_reproduce: { type: 'string', description: 'Etapes de repro precises (agent ou humain)' },
    root_cause: { type: 'string', description: 'Ou se situe le bug, fichiers/fonctions suspects' },
  },
}
const TEST_SCHEMA = {
  type: 'object', required: ['verdict', 'evidence'],
  properties: {
    verdict: { type: 'string', enum: ['Pass', 'Fail', 'Minor_changes'] },
    evidence: { type: 'string', description: 'Ce que tu as fait, observe, et justification du verdict' },
  },
}

// ---- Phase 1 : Triage (Debugger) ----
phase('Triage')
const triage = await agent(
  `NE PAS ECRIRE DE CODE. Reproduis et trie ce probleme signale :\n\n${bugReport}\n\n` +
  `C'est le codebase PDO lui-meme. Reproduis le bug : tu peux soit piloter le frontend via les outils MCP chrome-devtools si l'app web tourne, ` +
  `soit reproduire au niveau code/donnees (inspecter le YAML library sauvegarde et le code de (de)serialisation des Port/PortType). ` +
  `Tranche : Bug, Feature, Unsure ou Not Reproduced. Si c'est un bug, dis precisement d'ou il vient (fichiers, structs, champs serde) et ecris des etapes de repro reproductibles.`,
  { label: 'debugger', phase: 'Triage', schema: TRIAGE_SCHEMA }
)

if (!triage || triage.verdict !== 'Bug') {
  log(`Verdict triage : ${triage && triage.verdict || 'aucun'} -- pas un bug, sortie avant la boucle (= edge switch->End).`)
  return { triage, fixed: false, reason: 'not-a-bug' }
}

// ---- Phase 2 : boucle fix<->test bornee a 5 ----
phase('Fix loop')
const MAX_ITER = 5
let lastFailure = null, shipped = false, rounds = 0

for (let iter = 1; iter <= MAX_ITER; iter++) {
  rounds = iter
  await agent(
    `Implemente l'INTEGRALITE du correctif pour ce bug. Ne differe aucun travail, le perimetre a ete decide en amont.\n\n` +
    `Bug :\n${bugReport}\n\nEtapes de repro :\n${triage.how_to_reproduce}` +
    (triage.root_cause ? `\n\nCause racine suspectee :\n${triage.root_cause}` : '') +
    (lastFailure ? `\n\nLa tentative precedente a ECHOUE. Retour du Tester a corriger :\n${lastFailure}` : '') +
    `\n\nCorrige a la racine (la persistance du port_type), pas un contournement. ` +
    `Aucun test unitaire en echec ne doit etre tolere, meme preexistant.`,
    { label: `implementer#${iter}`, phase: 'Fix loop' }
  )

  const test = await agent(
    `Valide l'implementation pour ce bug.\n` +
    `1. Build le projet (cargo build -p pdo-daemon, et le frontend si touche) ; toute erreur de compilation = Fail.\n` +
    `2. Verifie que le bug est corrige. Pour ce bug de persistance, la verification naturelle est un test de round-trip serde : ` +
    `un Port avec port_type=ImageList serialise en YAML puis deserialise doit rester ImageList, et le snapshot de run doit preserver le type.\n` +
    `   AJUSTEMENT ASSUME (hors contexte PDO) : tu PEUX lancer un test cible "cargo test -p pdo-daemon <nom_du_test>" s'il porte sur de la logique isolee (round-trip serde) et ne construit PAS de daemon.\n` +
    `   INTERDIT quand meme : "cargo test"/"cargo test --workspace"/"cargo nextest" en entier, demarrer un "pdo daemon", tout process long en arriere-plan.\n` +
    `3. Si l'app frontend tourne deja, tu peux confirmer visuellement via chrome-devtools MCP (optionnel ici).\n\n` +
    `Verdict Pass uniquement si le bug est corrige ET le build propre.`,
    { label: `tester#${iter}`, phase: 'Fix loop', schema: TEST_SCHEMA }
  )

  log(`Tour ${iter} : verdict tester = ${test && test.verdict || 'aucun'}`)
  if (test && test.verdict === 'Pass') { shipped = true; break }
  lastFailure = (test && test.evidence) || 'Aucune evidence retournee.'
}

// ---- Phase 3 : Ship (commit-only, branche courante) ----
phase('Ship')
if (!shipped) {
  log(`${MAX_ITER} tours sans Pass -- JE NE COMMIT PAS (deviation assumee vs PDO qui irait quand meme a Ship It).`)
  return { triage, fixed: false, rounds, reason: 'max-iter-no-pass' }
}
const ship = await agent(
  `Le correctif a passe la verification. Commit sur la BRANCHE COURANTE (ne change pas de branche, ne merge PAS sur main, ne rebuild pas, ne supprime rien).\n\n` +
  `IMPORTANT : "git add" UNIQUEMENT les fichiers que tu as modifies pour ce correctif. ` +
  `Ne stage SURTOUT PAS les fichiers deja modifies sans rapport (frontend/vite.config.ts, package.json, package-lock.json, Makefile) -- laisse-les tels quels.\n\n` +
  `Message de commit clair (bug + correctif). Rends la branche + le hash du commit, le diff resume, et explique comment build/relancer l'app pour tester.`,
  { label: 'ship-it', phase: 'Ship' }
)
return { triage, fixed: true, rounds, ship }