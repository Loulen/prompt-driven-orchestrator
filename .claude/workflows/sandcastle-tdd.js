export const meta = {
  name: 'sandcastle-tdd',
  description:
    "Mimique sandcastle en workflow dynamique : vagues plan -> implement(/tdd) + review -> merge -> tester niveau 5 -> promote. Implementer pilote par la skill /tdd ; un tester agentique joue les scenarios couche 5 (docs/testing/scenarios) a la fin de CHAQUE vague. UNE SEULE branche de consolidation FIXE reutilisee a travers les runs (defaut sandcastle/integration, surchargeable args.target) — plus de wf/<tag>/integration jetable par run. Apres une vague VERTE (build+tests+L5), le workflow fait avancer main en fast-forward (args.promote=false pour ne livrer que sur la branche de consolidation). Cleanup garanti (try/finally).",
  phases: [
    { title: 'Bootstrap',        detail: "Reutilise (ou cree) la branche de consolidation FIXE + son worktree stable ; empile sur son tip" },
    { title: 'Plan',             detail: 'Agent lit les issues ready-for-agent, sort les non-bloquees (lecture seule)' },
    { title: 'Prepare',          detail: 'Cree en SERIE un worktree par issue depuis le tip integration (zero race)' },
    { title: 'Implement+Review', detail: 'Par issue (parallele) : implementer /tdd dans son worktree, puis reviewer dans le meme' },
    { title: 'Merge',            detail: 'Merge des branches completees DANS la branche de consolidation (worktree dedie) + build' },
    { title: 'Test L5',          detail: 'Tester agentique : scenarios couche 5 sur le worktree integration (best-effort, app live + navigateur MCP)' },
    { title: 'Fix L5',           detail: 'Sur FAIL L5 : fix-loop borne (cap L5_FIX_TRIES) qui corrige le CODE PRODUIT (jamais le scenario) + re-test ; fallback surface-humaine' },
    { title: 'Promote',          detail: 'Vague verte : avance main (FF-only) jusqu\'au tip de consolidation ; note le SHA d\'avant pour reset trivial. Jamais de fusion divergente, jamais de push' },
    { title: 'Cleanup',          detail: 'Garanti : seul proprietaire des worktrees feature ; supprime branches feature mergees sur succes, garde tout sur echec ; CONSERVE la branche de consolidation + son worktree (durables)' },
  ],
}

// ---------------------------------------------------------------------------
// Options (args). Defauts surs : rien de sortant (pas de push, pas de PR, pas
// de fermeture d'issue). On ne touche jamais la branche/checkout de l'utilisateur.
// ---------------------------------------------------------------------------
// `args` peut arriver soit comme objet JSON, soit comme CHAINE (le tool Workflow
// delivre parfois un objet sous forme de chaine JSON). On parse si c'est une chaine ;
// une chaine non-JSON (p.ex. "issues en ready-for-agent") retombe sur les defauts.
// Le harness peut DOUBLE-encoder args (chaine-JSON d'une chaine-JSON) : on deballe
// en boucle jusqu'a obtenir un objet (ou une chaine non-JSON => defauts).
let _rawArgs = args
let _g = 0
while (typeof _rawArgs === 'string' && _g++ < 5) {
  try { _rawArgs = JSON.parse(_rawArgs) } catch (e) { break }
}
const opt           = (_rawArgs && typeof _rawArgs === 'object') ? _rawArgs : {}
const LABEL         = opt.label             || 'ready-for-agent' // label des issues a prendre
const MAX_WAVES     = opt.max_waves         || 3                 // garde-fou : nb max de vagues
const WT_ROOT       = opt.worktree_root     || '.pdo/wf'     // racine des worktrees jetables
const TARGET        = opt.target            || 'sandcastle/integration' // branche de consolidation FIXE, reutilisee + empilee a travers les runs (jamais une branche neuve par run)
const INTEG_WT      = opt.integration_worktree || `${WT_ROOT}/integration` // worktree STABLE de la branche de consolidation (un seul, pas par run_tag)
const SOURCE        = opt.source            || ''               // base a la PREMIERE creation de TARGET seulement (defaut: main). Sans effet si TARGET existe deja (on empile sur son tip).
const PROMOTE       = opt.promote !== false                     // avancer PROMOTE_INTO (FF-only) apres une vague VERTE (defaut: true)
const PROMOTE_INTO  = opt.promote_into      || 'main'           // branche locale a faire avancer en fast-forward (defaut: main)
const SKIP_TEST     = opt.skip_test         || false             // sauter la couche 5
const STOP_ON_FAIL  = opt.stop_on_fail !== false                 // stopper les vagues si L5 FAIL (defaut: true)
const CLOSE_ISSUES  = opt.close_issues      || false             // SORTANT : fermer les issues au merge
const REPO_HINT     = opt.repo              || 'Loulen/prompt-driven-orchestrator'  // repo gh (cf CLAUDE.local.md)
const WAVE_FLOOR    = opt.wave_floor        || 80_000            // marge tokens avant de lancer une vague
const PRIORITY      = Array.isArray(opt.priority) ? opt.priority.map(Number) : []  // numeros d'issues a traiter en priorite (ordre)
const NOTES         = (opt.notes && typeof opt.notes === 'object') ? opt.notes : {} // {"<num>": "consigne supplementaire pour l'implementer de cette issue"}
const L5_FIX_TRIES  = Number.isInteger(opt.l5_fix_tries) ? opt.l5_fix_tries : 2     // fix-loop borne apres un FAIL couche 5 (0 = desactive)

// ---------------------------------------------------------------------------
// Schemas (sortie structuree, validee au tool-call -> pas de parsing)
// ---------------------------------------------------------------------------
const BOOTSTRAP_SCHEMA = {
  type: 'object', required: ['run_tag', 'source_branch', 'integration_branch', 'integration_worktree', 'tip_sha'],
  properties: {
    run_tag:              { type: 'string', description: 'Tag de run unique (genere via shell, p.ex. $(date +%s)-$RANDOM) — namespace les worktrees feature ephemeres' },
    source_branch:        { type: 'string', description: 'Branche actuellement checkout (lecture seule, jamais modifiee)' },
    source_sha:           { type: 'string' },
    integration_branch:   { type: 'string', description: "Branche de consolidation FIXE reutilisee (= TARGET, defaut sandcastle/integration)" },
    integration_worktree: { type: 'string', description: "Worktree (stable) de la branche de consolidation, tel qu'effectivement utilise" },
    tip_sha:              { type: 'string', description: "Tip actuel de la branche de consolidation (apres reutilisation/creation)" },
    reused:               { type: 'boolean', description: 'true si TARGET existait deja et a ete reutilisee ; false si creee ce run' },
  },
}
const PLAN_SCHEMA = {
  type: 'object', required: ['issues'],
  properties: {
    issues: {
      type: 'array',
      description: 'Issues NON bloquees, travaillables en parallele cette vague. Vide si rien a faire.',
      items: {
        type: 'object', required: ['number', 'title', 'branch', 'slug'],
        properties: {
          number: { type: 'number' },
          title:  { type: 'string' },
          branch: { type: 'string', description: 'sandcastle/<run_tag>/issue-<number>-<slug>' },
          slug:   { type: 'string', description: 'slug court sans slash (chemin de worktree)' },
        },
      },
    },
  },
}
const PREPARE_SCHEMA = {
  type: 'object', required: ['wave_base_sha', 'prepared'],
  properties: {
    wave_base_sha: { type: 'string', description: 'Tip integration AVANT cette vague (pour le diff du tester)' },
    prepared: {
      type: 'array',
      items: {
        type: 'object', required: ['number', 'branch', 'worktree', 'ready'],
        properties: {
          number:   { type: 'number' },
          title:    { type: 'string' },
          branch:   { type: 'string' },
          worktree: { type: 'string' },
          ready:    { type: 'boolean', description: 'worktree cree et utilisable' },
        },
      },
    },
  },
}
const IMPL_SCHEMA = {
  type: 'object', required: ['branch', 'worktree', 'committed'],
  properties: {
    branch:    { type: 'string' },
    worktree:  { type: 'string' },
    committed: { type: 'boolean', description: 'true SEULEMENT si au moins un commit existe vs le tip integration' },
    summary:   { type: 'string' },
    blockers:  { type: 'string' },
  },
}
const REVIEW_SCHEMA = {
  type: 'object', required: ['branch', 'changed'],
  properties: {
    branch:  { type: 'string' },
    changed: { type: 'boolean', description: 'true si le reviewer a commit des ameliorations' },
    notes:   { type: 'string' },
  },
}
const MERGE_SCHEMA = {
  type: 'object', required: ['merged_branches', 'build_ok', 'integration_head_sha'],
  properties: {
    merged_branches:      { type: 'array', items: { type: 'string' } },
    conflicts:            { type: 'boolean' },
    build_ok:             { type: 'boolean', description: 'build + tests verts apres merge' },
    integration_head_sha: { type: 'string' },
    summary:              { type: 'string' },
  },
}
const TEST_SCHEMA = {
  type: 'object', required: ['verdict', 'setup_ok', 'scenarios'],
  properties: {
    verdict:  { type: 'string', enum: ['PASS', 'FAIL', 'SKIPPED'] },
    setup_ok: { type: 'boolean', description: 'app couche 5 demarree et pilotable' },
    port:     { type: 'number', description: 'port non-defaut effectivement utilise' },
    scenarios: {
      type: 'array',
      items: {
        type: 'object', required: ['name', 'verdict'],
        properties: {
          name:     { type: 'string' },
          verdict:  { type: 'string', enum: ['PASS', 'FAIL', 'SKIPPED'] },
          evidence: { type: 'array', items: { type: 'string' } },
        },
      },
    },
    anomalies: { type: 'array', items: { type: 'string' } },
  },
}
const FIX_SCHEMA = {
  type: 'object', required: ['kind', 'committed'],
  properties: {
    kind:                 { type: 'string', enum: ['fixed', 'unfixable-scope', 'gave-up'], description: 'fixed=code produit corrige ; unfixable-scope=la regression exige une decision hors-perimetre (humain/autre issue) ; gave-up=cause non trouvee' },
    committed:            { type: 'boolean', description: 'true SEULEMENT si un commit de fix (code produit, jamais le scenario) a ete cree dans le worktree integration' },
    integration_head_sha: { type: 'string', description: 'nouveau HEAD integration apres le fix' },
    touched:              { type: 'array', items: { type: 'string' }, description: 'fichiers modifies (DOIT exclure docs/testing/scenarios/**)' },
    summary:              { type: 'string' },
  },
}
const PROMOTE_SCHEMA = {
  type: 'object', required: ['advanced', 'method'],
  properties: {
    advanced:       { type: 'boolean', description: 'true si la branche promue a effectivement avance' },
    method:         { type: 'string', enum: ['ff-ref', 'ff-worktree', 'skipped-not-ff', 'skipped-blocked', 'skipped-error'], description: 'ff-ref=ref avancee (branche non checkout) ; ff-worktree=FF dans le worktree qui la porte (checkout principal inclus) ; skipped-not-ff=divergence (pas un FF) ; skipped-blocked=FF refuse par git (arbre sale en conflit) ; skipped-error=autre' },
    target_old_sha: { type: 'string', description: 'SHA de la branche promue AVANT — point de revert (git reset --hard <ce sha>)' },
    target_new_sha: { type: 'string', description: 'SHA apres (= tip de consolidation si avance)' },
    manual_cmd:     { type: 'string', description: 'commande FF a lancer manuellement si non avance (sinon vide)' },
    summary:        { type: 'string' },
  },
}
const CLEANUP_SCHEMA = {
  type: 'object', required: ['removed', 'kept'],
  properties: {
    removed: { type: 'array', items: { type: 'string' }, description: 'worktrees/branches reellement supprimes' },
    kept:    { type: 'array', items: { type: 'string' }, description: 'worktrees/branches conserves (echec ou integration)' },
    failed:  { type: 'array', items: { type: 'string' }, description: 'suppressions qui ont echoue (a nettoyer a la main)' },
  },
}

// ---------------------------------------------------------------------------
// Prompts (inline, self-contained). Les agents font TOUT le git/shell :
// le script lui-meme n'a ni fs ni shell.
// ---------------------------------------------------------------------------
const bootstrapPrompt = () =>
  `NE PAS ECRIRE DE CODE APPLICATIF. Tu prepares l'environnement de consolidation du run.\n` +
  `Le checkout principal de l'utilisateur (branche courante + d'eventuels changements non commites) est INTOUCHABLE : ne le checkout pas, ne le stash pas, ne le modifie pas. Lis juste sa branche courante pour info : \`git rev-parse --abbrev-ref HEAD\` et son SHA (source_branch, source_sha).\n\n` +
  `1. Genere un tag de run UNIQUE (namespace les worktrees feature ephemeres, pas la branche) : run_tag="$(date +%s)-$RANDOM".\n` +
  `2. \`git worktree prune\` (nettoie les refs de worktrees disparus).\n` +
  `3. Branche de consolidation FIXE et REUTILISEE = \`${TARGET}\`. NE CREE JAMAIS une branche neuve par run (pas de wf/<tag>/...). Determine son etat :\n` +
  `   a. Existe-t-elle ? \`git rev-parse --verify --quiet refs/heads/${TARGET}\`.\n` +
  `   b. Est-elle deja attachee a un worktree ? \`git worktree list --porcelain\` (cherche une ligne \`branch refs/heads/${TARGET}\`).\n` +
  `4. Choisis le worktree de consolidation (renvoie le chemin REEL dans integration_worktree) :\n` +
  `   - Si \`${TARGET}\` est DEJA attachee a un worktree W : REUTILISE W tel quel. Verifie qu'il est propre (\`git -C W status --porcelain\` vide) ; s'il reste des modifs non commitees d'un run anterieur interrompu, c'est anormal — renvoie-le dans summary mais n'efface rien d'office.\n` +
  `   - Sinon, si \`${TARGET}\` existe mais sans worktree : attache-la au chemin stable \`${INTEG_WT}\` : \`git worktree add ${INTEG_WT} ${TARGET}\` (si le chemin existe deja en doublon casse, \`git worktree prune\` puis reessaie).\n` +
  `   - Sinon (\`${TARGET}\` n'existe pas) : cree-la a partir de la base ` + (SOURCE ? `\`${SOURCE}\` (fournie explicitement)` : `\`${PROMOTE_INTO}\` (la branche promue par defaut)`) + ` : \`git rev-parse <base>\` puis \`git worktree add -b ${TARGET} ${INTEG_WT} <base_sha>\`.\n` +
  `   ATTENTION : si \`${TARGET}\` etait par erreur la branche du checkout principal, git refuserait un 2e worktree — ce n'est pas censé arriver (TARGET est dediee) ; si ça arrive, renvoie l'erreur dans summary sans toucher au checkout.\n` +
  `5. Dans le worktree de consolidation, assure les pre-requis (cf memoire projet) : si \`.pdo/pipelines\` manque, copie-le depuis le checkout principal ; le frontend sera builde plus tard par les agents qui en ont besoin.\n` +
  `Renvoie run_tag, source_branch, source_sha, integration_branch=\`${TARGET}\`, integration_worktree (chemin REEL utilise), tip_sha (= \`git -C <worktree> rev-parse HEAD\`), et reused (true si ${TARGET} preexistait).`

const planPrompt = (exclude) =>
  `NE PAS ECRIRE DE CODE. Tu es le PLANNER (role sandcastle), en LECTURE SEULE.\n` +
  `1. Lis les issues ouvertes labellisees "${LABEL}" : \`gh issue list --repo ${REPO_HINT} --state open --label ${LABEL} --json number,title,body,labels,comments\`.\n` +
  `2. Graphe de dependances : B est BLOQUEE par A si B requiert du code/infra que A introduit, si A et B touchent des fichiers/modules qui se chevauchent (conflit probable), ou si B depend d'une decision/API que A va etablir.\n` +
  `3. NON bloquee = aucune dependance bloquante sur une autre issue ouverte. Une issue qui ressemble a un PRD avec des sous-issues d'implementation ne peut pas etre travaillee.\n` +
  (exclude && exclude.length
    ? `   IGNORE les issues DEJA TRAITEES ce run (deja integrees, ne les re-propose pas) : ${exclude.map((n) => '#' + n).join(', ')}.\n`
    : '') +
  `4. Pour chaque issue non bloquee : branche \`sandcastle/<run_tag>/issue-<number>-<slug>\` (utilise EXACTEMENT le run_tag fourni) et un slug court sans slash.\n` +
  `   run_tag de ce run : "{{RUN_TAG}}".\n` +
  (PRIORITY.length
    ? `5. MODE PRIORITE (ordre IMPOSE : ${PRIORITY.map((n) => '#' + n).join(' > ')}). Le demandeur a DEJA valide cet ordre et l'absence de dependance bloquante entre ces issues. Donc TANT QUE ces issues ne sont pas toutes traitees :\n` +
      `   - IGNORE COMPLETEMENT ta propre analyse de blocage POUR ces issues (traite-les comme non bloquees, meme si elles chevauchent d'autres fichiers).\n` +
      `   - Renvoie EXACTEMENT UNE issue cette vague = la PREMIERE de la liste ci-dessus qui n'est ni deja traitee ni fermee, et RIEN d'autre (n'ajoute AUCUNE issue hors-liste tant que la liste n'est pas epuisee).\n` +
      `   - Ce n'est qu'une fois TOUTES les issues de la liste traitees que tu reprends l'analyse normale (multi-issues non bloquees).\n`
    : '') +
  `Si tout est bloque, ne renvoie que la candidate la plus prioritaire (non deja traitee). Si rien de nouveau n'est travaillable, renvoie issues=[].`

const preparePrompt = (issues, integrationBranch, integrationWorktree) =>
  `NE PAS ECRIRE DE CODE. Tu prepares les worktrees de la vague, EN SERIE (un par un, jamais en parallele — evite toute race sur .git/worktrees).\n` +
  `Base = le TIP ACTUEL de la branche d'integration \`${integrationBranch}\` (worktree ${integrationWorktree}). ` +
  `Recupere ce SHA d'abord : \`git -C ${integrationWorktree} rev-parse HEAD\` -> c'est \`wave_base_sha\` (a renvoyer, le tester en aura besoin).\n\n` +
  `Pour CHAQUE issue ci-dessous, dans l'ordre, cree un worktree base sur wave_base_sha avec EXACTEMENT le chemin et la branche indiques :\n` +
  issues.map((i) => `  - #${i.number} (${i.title}) : git worktree add -b ${i.branch} ${i.worktree} <wave_base_sha>`).join('\n') + `\n\n` +
  `Dans chaque worktree, assure les pre-requis : si \`.pdo/pipelines\` manque, copie-le depuis le checkout principal. ` +
  `Si un \`git worktree add\` echoue (branche deja existante d'un run anterieur, etc.), marque \`ready:false\` pour cette issue et continue les autres.\n` +
  `Renvoie wave_base_sha et la liste \`prepared\` (number, title, branch, worktree, ready).`

const implPrompt = (p, integrationBranch) =>
  `Tu es l'IMPLEMENTER. Issue #${p.number} UNIQUEMENT : ${p.title}.\n\n` +
  `## Worktree (deja cree)\n` +
  `Travaille EXCLUSIVEMENT dans \`${p.worktree}\` (branche ${p.branch}). \`cd ${p.worktree}\` (ou \`git -C ${p.worktree}\`). ` +
  `Ne cree pas de worktree, ne change pas de branche, ne touche a aucun autre worktree ni au checkout principal.\n\n` +
  `## Skill obligatoire : /tdd\n` +
  `Pilote ton travail avec la skill **/tdd**. Si elle n'est pas chargeable, lis \`~/.agents/skills/tdd/SKILL.md\` (+ annexes). ` +
  `Si NI la skill NI le fichier ne sont accessibles, applique quand meme ce contrat : red-green-refactor en SLICES VERTICALES — UN test -> l'implem minimale -> repeter ; jamais tous les tests d'abord ; tests sur le comportement via l'interface publique, pas l'implementation ; refactor seulement a vert.\n\n` +
  `## Contexte (source de verite, depuis ton worktree)\n` +
  `\`gh issue view ${p.number} --repo ${REPO_HINT} --comments\` (+ PRD parent si lie). Lis \`./CONTEXT.md\`, \`./.sandcastle/CODING_STANDARDS.md\` (conventions + commandes build/test), et les ADR pertinents de \`./docs/adr/\`. Les criteres d'acceptation definissent "done".\n\n` +
  `## Feedback loops\n` +
  `Avant de commit, lance les commandes pertinentes de CODING_STANDARDS (Rust si tu touches du Rust, frontend si frontend, les deux si les deux). Aucun test en echec tolere.\n\n` +
  `## Scenarios couche 5 (synchro UNIQUEMENT si TON changement les invalide)\n` +
  `Les scenarios \`docs/testing/scenarios/*.md\` sont des tests d'acceptation joues a la fin de la vague. Si ton implementation modifie DELIBEREMENT (acceptance criteria de CETTE issue) un comportement/une UI qu'un scenario existant asserte — p.ex. tu retires un bouton de toolbar, renommes/supprimes un selecteur ou data-testid, changes un flux d'interaction — METS A JOUR l'assertion CONCERNEE pour refleter le nouveau comportement voulu (cite l'issue/ADR dans le message de commit). C'est dans le perimetre de ton issue : un scenario qui asserte l'ancien comportement provoquerait un FAIL L5 faux-positif qui bloque la vague.\n` +
  `INTERDIT (anti-triche, la review + le tester L5 le rejettent) : affaiblir/supprimer une assertion SANS rapport avec ton changement, ou vider un scenario pour le faire passer. Tu ne touches la mesure QUE quand c'est TON changement deliberé qui rend l'ancienne assertion obsolete, et tu la remplaces par l'assertion du NOUVEAU comportement (pas par rien).\n\n` +
  `## Commit\n` +
  `Commit dans ton worktree, message prefixe \`RALPH:\`, \`refs #${p.number}\`, ce qui a ete fait + decisions + fichiers + blocages. NE FERME PAS l'issue. Si incomplet, commente l'issue.\n\n` +
  `## Sortie\n` +
  `committed=true SEULEMENT si \`git -C ${p.worktree} log ${integrationBranch}..HEAD --oneline\` est non vide. Sinon committed=false + blockers explicites.\n` +
  `PERIMETRE STRICT : QUE cette issue (sharp-tool), aucun refactor opportuniste.` +
  (NOTES[String(p.number)]
    ? `\n\n## Consigne specifique a cette issue (OVERRIDE — prime sur le body de l'issue en cas de conflit)\n${NOTES[String(p.number)]}`
    : '')

const reviewPrompt = (impl, integrationBranch) =>
  `Tu es le REVIEWER. Worktree EXISTANT : ${impl.worktree} (branche ${impl.branch}). \`cd ${impl.worktree}\`. Ne cree pas de worktree, ne change pas de branche.\n\n` +
  `## Diff a relire\n` +
  `\`git -C ${impl.worktree} diff ${integrationBranch}...${impl.branch}\` et \`git -C ${impl.worktree} log ${integrationBranch}..${impl.branch} --oneline\`.\n\n` +
  `## Process\n` +
  `1. Comprendre l'intention (diff + commits + criteres de l'issue).\n` +
  `2. Correctness : implem conforme a l'intention ? edge cases ? comportements nouveaux couverts par des tests ? casts douteux / any / hypotheses non verifiees ? injection / fuite de secret ?\n` +
  `3. Clarte/maintenabilite : moins de complexite et d'imbrication, pas de redondance, bons noms, pas de ternaires imbriques. PRESERVE le comportement exact (seul le "comment" change).\n` +
  `4. Standards : ./.sandcastle/CODING_STANDARDS.md.\n\n` +
  `## Execution\n` +
  `Si ameliorations : applique-les dans le worktree, relance build+tests pertinents, commit \`RALPH: Review - ...\`. Sinon ne fais rien. changed=true seulement si tu as commit.`

const mergePrompt = (completed, integrationBranch, integrationWorktree) =>
  `Tu es le MERGER. Tu opere UNIQUEMENT dans le worktree d'integration \`${integrationWorktree}\` (branche ${integrationBranch}). ` +
  `Tu ne touches NI le checkout principal NI la branche de l'utilisateur. Pas de push, pas de PR.\n\n` +
  `## Branches a merger (DANS CET ORDRE — ne reordonne pas)\n` +
  completed.map((c) => `- ${c.branch}  — issue #${c.number}: ${c.title}`).join('\n') + `\n\n` +
  `## Procedure\n` +
  `1. Assure-toi d'etre sur ${integrationBranch} dans ${integrationWorktree} (\`git -C ${integrationWorktree} status\`).\n` +
  `2. Pour chaque branche, dans l'ordre : \`git -C ${integrationWorktree} merge <branche> --no-edit\`. Conflit -> lis les deux cotes, resous intelligemment.\n` +
  `3. Apres CHAQUE merge : build + tests pertinents (cf CODING_STANDARDS). Rouge -> corrige avant la branche suivante.\n` +
  `4. Un commit de synthese a la fin si necessaire.\n` +
  `5. NE supprime PAS les worktrees ni les branches (le cleanup final en est le SEUL proprietaire).\n` +
  (CLOSE_ISSUES
    ? `6. SORTANT — ferme les issues mergees : \`gh issue close <n> --repo ${REPO_HINT}\` (+ PRD parent si complete).\n`
    : `6. NE FERME PAS les issues (gate humaine).\n`) +
  `\nRenvoie merged_branches (celles reellement mergees), conflicts (bool), build_ok (bool), integration_head_sha (= \`git -C ${integrationWorktree} rev-parse HEAD\`), et un resume.`

const testPrompt = (waveBaseSha, merge, completed, integrationWorktree) =>
  `Tu es le TESTER COUCHE 5 (scenarios agentiques). Cible : le worktree d'integration \`${integrationWorktree}\` APRES merge de la vague (head ${merge.integration_head_sha || '?'}).\n` +
  `Protocole de reference a suivre A LA LETTRE : \`docs/agents/run-scenario.md\` (setup -> piloter le navigateur -> valider les effets de bord -> verdict JSON). Couche definie dans \`docs/adr/0004-testing-strategy.md\`.\n\n` +
  `## Selection des scenarios (deterministe)\n` +
  `Diff de la vague = \`git -C ${integrationWorktree} diff ${waveBaseSha}..${merge.integration_head_sha || 'HEAD'} --stat\` (SHAs explicites, n'utilise PAS de reflog @{1}).\n` +
  `Liste TOUS les scenarios : \`ls ${integrationWorktree}/docs/testing/scenarios/*.md\`, lis chacun, et choisis ceux dont la portee CHEVAUCHE le diff. Au minimum, joue \`run-minimal\` (smoke du parcours principal). Le set evolue — DECOUVRE-le via le \`ls\` (ne te fie pas a une liste figee).\n\n` +
  `## Setup (best-effort, NON DESTRUCTIF)\n` +
  `La couche 5 a besoin de l'app qui TOURNE (depuis ${integrationWorktree}) : build frontend (\`frontend/dist\`), build daemon (\`cargo build -p pdo-daemon\`), demarrage du daemon, navigateur via MCP chrome-devtools (sinon playwright MCP), tmux + \`claude\` pour run-minimal.\n` +
  `PORT — ne casse JAMAIS l'environnement de l'utilisateur :\n` +
  `  - N'utilise PAS le port par defaut 5172. D'abord verifie que le daemon supporte un port custom : \`pdo --help\` / \`pdo daemon --help\` (flag \`--port\` ou env \`PDO_PORT\`). Si tu ne peux pas confirmer le support, verdict SKIPPED.\n` +
  `  - Choisis un port LIBRE (verifie avec \`ss -ltn\` / un curl qui echoue) hors 5172, demarre le daemon dessus, et CONFIRME : \`curl -fs http://127.0.0.1:<port>/runs\` renvoie du JSON. Adapte TOUTES les URLs/ports des scenarios (5172/5173) a ton port.\n` +
  `  - Si 5172 est occupe, ne tue rien : c'est probablement le daemon de l'utilisateur.\n` +
  `  - GARDE le PID du daemon que TU demarres ; a la fin, archive les Run que tu as lances (\`POST /runs/<id>/commands {"kind":"cleanup_run"}\`) PUIS tue CE PID (et lui seul). Ne touche a aucun process que tu n'as pas demarre.\n` +
  `  - Toolchain manquant (pas de cargo/tmux) ou build qui echoue -> verdict SKIPPED (pas FAIL), explique dans evidence.\n` +
  `  - MEME si setup_ok=false ou verdict SKIPPED : avant de rendre la main, tue tout daemon que TU aurais demarre (ne laisse aucun process orphelin). Le cleanup final ne gere PAS les process, seulement les worktrees/branches.\n\n` +
  `## Verdict\n` +
  `Pour chaque scenario lance, mappe son format de verdict declare vers {name, verdict, evidence}. Une assertion en echec = ce scenario FAIL (pas de demi-PASS). ` +
  `verdict global = PASS si tous les scenarios lances passent ; FAIL si >=1 FAIL (vraie regression) ; SKIPPED si l'app n'a pas pu etre pilotee. Renvoie aussi le port utilise.`

const l5FixPrompt = (test, integrationBranch, integrationWorktree, attempt, maxAttempts) =>
  `Tu es le FIXER COUCHE 5 (tentative ${attempt}/${maxAttempts}). La couche 5 a trouve une REGRESSION apres merge dans l'integration. ` +
  `Tu corriges le CODE PRODUIT pour que le(s) scenario(s) passent HONNETEMENT. Tu opere UNIQUEMENT dans le worktree d'integration \`${integrationWorktree}\` (branche ${integrationBranch}). Pas de push, pas de PR, pas de fermeture d'issue.\n\n` +
  `## Ce que la couche 5 reproche (source de verite du QUOI corriger)\n` +
  `Scenarios FAIL : ${((test && test.scenarios) || []).filter((s) => s.verdict === 'FAIL').map((s) => s.name).join(', ') || '(voir anomalies)'}.\n` +
  '```json\n' + JSON.stringify({ scenarios: ((test && test.scenarios) || []).filter((s) => s.verdict === 'FAIL'), anomalies: (test && test.anomalies) || [] }, null, 2).slice(0, 4500) + '\n```\n\n' +
  `## Regles ABSOLUES (anti-triche)\n` +
  `- INTERDIT d'editer, affaiblir ou supprimer un fichier de scenario \`docs/testing/scenarios/**\`, ni d'affaiblir/supprimer une assertion de test pour forcer le vert. Tu corriges le PRODUIT, jamais la mesure. (Le champ touched[] que tu renvoies DOIT exclure docs/testing/scenarios/**.)\n` +
  `- Si la regression n'est PAS corrigible dans le perimetre de la vague (elle exige une decision d'architecture, une autre issue, un reordonnancement, un choix produit), NE bricole RIEN : renvoie kind="unfixable-scope" + un summary expliquant la decision a prendre. Surface au gate humain > fix incoherent.\n` +
  `- Perimetre minimal : juste de quoi lever cette regression. Aucun refactor opportuniste.\n\n` +
  `## Procedure\n` +
  `1. Comprends l'echec depuis l'evidence ci-dessus ; localise la cause dans le code PRODUIT (pas le test).\n` +
  `2. Corrige, puis relance les feedback loops pertinents (cf ./.sandcastle/CODING_STANDARDS.md : build + tests Rust et/ou frontend selon les fichiers touches). Aucun test en echec tolere.\n` +
  `3. Commit dans le worktree integration, message \`RALPH: L5-fix - <quoi>\` (ce commit court-circuite la review per-issue ; le gate humain le relira).\n` +
  `4. NE supprime PAS de worktree/branche.\n\n` +
  `## Sortie\n` +
  `kind, committed (true seulement si commit cree), integration_head_sha (= \`git -C ${integrationWorktree} rev-parse HEAD\`), touched[] (hors scenarios), summary.`

const promotePrompt = (integrationBranch, integrationHeadSha, into) =>
  `Tu es le PROMOTER. La vague est VERTE (build + tests + couche 5 OK). Objectif : faire avancer la branche locale \`${into}\` jusqu'au tip de \`${integrationBranch}\` (${integrationHeadSha}) en FAST-FORWARD UNIQUEMENT. ` +
  `JAMAIS de fusion divergente, jamais de resolution de conflit, jamais de push, jamais de --force/-f sur une ref divergente.\n\n` +
  `## Procedure\n` +
  `1. Note le SHA actuel de \`${into}\` -> \`target_old_sha\` (point de revert : \`git reset --hard <ce sha>\`).\n` +
  `2. Verifie que c'est bien un fast-forward : \`git merge-base --is-ancestor ${into} ${integrationBranch}\` (exit 0 = FF possible). Si NON (\`${into}\` a des commits absents de la consolidation -> divergence) : N'AVANCE RIEN, method="skipped-not-ff", \`manual_cmd\` = la commande de merge a decider a la main, explique la divergence dans summary. STOP.\n` +
  `3. Localise ou \`${into}\` est checkout : \`git worktree list --porcelain\` (ligne \`branch refs/heads/${into}\`).\n` +
  `   - \`${into}\` checkout NULLE PART -> avance la ref directement : \`git branch -f ${into} ${integrationBranch}\`. method="ff-ref".\n` +
  `   - \`${into}\` checkout dans un worktree W (le checkout principal de l'utilisateur en fait partie — et c'est OK d'y faire un FF) -> \`git -C W merge --ff-only ${integrationBranch}\`. method="ff-worktree". ` +
  `Un FF met juste a jour l'arbre ; d'eventuels changements non commites qui n'entrent pas en conflit sont preserves par git. Si (et seulement si) git REFUSE le FF (arbre sale en conflit avec les fichiers avances) : N'INSISTE PAS, ne stash pas, ne commit pas a la place de l'utilisateur — method="skipped-blocked", \`manual_cmd\`=\`git -C W merge --ff-only ${integrationBranch}\` (a relancer une fois l'arbre propre), explique dans summary.\n` +
  `4. Ne touche a RIEN d'autre (pas d'autre branche, pas de remote).\n\n` +
  `## Sortie\n` +
  `advanced (bool), method, target_old_sha, target_new_sha (= \`git rev-parse ${into}\` apres), manual_cmd (vide si avance), summary.`

const cleanupPrompt = (runTag, integrationBranch, integrationWorktree, success, mergedBranches) =>
  `Tu es le CLEANUP (garanti, SEUL proprietaire des worktrees ; ne build pas, ne merge pas, ne push pas).\n` +
  `1. Inventaire d'abord : \`git worktree list\` et \`git branch --list 'sandcastle/${runTag}/*'\`. N'agis QUE sur les worktrees FEATURE de ce run (sous \`${WT_ROOT}/${runTag}/\`) et les branches feature \`sandcastle/${runTag}/*\`. La branche de consolidation \`${integrationBranch}\` et son worktree \`${integrationWorktree}\` sont DURABLES et PROTEGES : ne les supprime JAMAIS. Ne touche JAMAIS au checkout principal, ni a un worktree/branche hors du prefixe de ce run, ni a une branche promue (ex. main), ni a une branche distante.\n` +
  (success
    ? `2. SUCCES : pour chaque worktree FEATURE sous \`${WT_ROOT}/${runTag}/\` (jamais le worktree de consolidation \`${integrationWorktree}\`) qui existe -> \`git worktree remove <path>\` (--force seulement si refus attendu). Puis \`git worktree prune\`.\n` +
      `3. Supprime UNIQUEMENT les branches feature CONFIRMEES MERGEES : ${mergedBranches.map((b) => '`' + b + '`').join(', ') || '(aucune)'} -> \`git branch -D <b>\`. ` +
      `Toute autre branche \`sandcastle/${runTag}/*\` NON listee ici (merge non abouti, conflit) est a CONSERVER (elle peut porter du travail non integre).\n` +
      `4. CONSERVE la branche de consolidation \`${integrationBranch}\` ET son worktree \`${integrationWorktree}\` (durables, reutilises au prochain run — ne les retire pas).\n`
    : `2. ECHEC/incomplet : NE supprime AUCUNE branche ni worktree. Conserve tout (worktrees feature \`${WT_ROOT}/${runTag}/\`, branches \`sandcastle/${runTag}/*\`, consolidation \`${integrationBranch}\` + worktree) pour inspection humaine.\n`) +
  `Renvoie removed[], kept[], failed[] (suppressions qui ont echoue, a nettoyer a la main).`

// ---------------------------------------------------------------------------
// Orchestration : bootstrap -> vagues (plan -> prepare -> implement+review ->
// merge -> test L5) -> cleanup garanti (try/finally).
// ---------------------------------------------------------------------------
const waves = []
const allMergedBranches = []    // branches feature CONFIRMEES mergees (seules supprimables au cleanup)
const handled = []              // numeros d'issues deja traitees ce run (exclues des plans suivants)
let runOk = true
let boot = null

try {
  // ---- Bootstrap (une fois) ----
  phase('Bootstrap')
  boot = await agent(bootstrapPrompt(), { label: 'bootstrap', phase: 'Bootstrap', schema: BOOTSTRAP_SCHEMA })
  if (!boot || !boot.run_tag || !boot.integration_worktree) {
    log('Bootstrap a echoue (pas de worktree d\'integration) — abandon.')
    runOk = false
    return { ok: false, reason: 'bootstrap-failed', boot }
  }
  const { run_tag, integration_branch, integration_worktree } = boot
  log(`Run tag ${run_tag} — consolidation \`${integration_branch}\` ${boot.reused ? 'REUTILISEE' : 'creee'} @ ${(boot.tip_sha || '').slice(0, 9)} (checkout \`${boot.source_branch}\` jamais modifie). ${PROMOTE ? `Vagues vertes -> FF \`${PROMOTE_INTO}\`.` : 'Promote desactive (livraison sur la branche de consolidation).'}`)

  let stop = false
  for (let wave = 1; wave <= MAX_WAVES && !stop; wave++) {
    if (budget.total && budget.remaining() < WAVE_FLOOR) {
      log(`Budget bas (${Math.round(budget.remaining() / 1000)}k restants) — arret avant la vague ${wave}.`)
      break
    }

    // ---- Plan (lecture seule) ----
    phase(`Vague ${wave} · Plan`)
    // Function replacer : run_tag est insere litteralement (pas d'interpretation de $&/$1).
    const plan = await agent(planPrompt(handled).replace('{{RUN_TAG}}', () => run_tag), {
      label: `planner·v${wave}`, phase: `Vague ${wave} · Plan`, schema: PLAN_SCHEMA,
    })
    if (!plan || !plan.issues || plan.issues.length === 0) {
      log(`Vague ${wave} : aucune issue travaillable — fin des vagues.`)
      break
    }
    // Chemins de worktree calcules par le script (uniques par run/vague/issue).
    const planned = plan.issues.map((i) => ({
      ...i, worktree: `${WT_ROOT}/${run_tag}/w${wave}-${i.slug}`,
    }))
    log(`Vague ${wave} : ${planned.length} issue(s) -> ` + planned.map((i) => `#${i.number}`).join(', '))

    // ---- Prepare (SERIE, zero race) ----
    phase(`Vague ${wave} · Prepare`)
    const prep = await agent(preparePrompt(planned, integration_branch, integration_worktree), {
      label: `prepare·v${wave}`, phase: `Vague ${wave} · Prepare`, schema: PREPARE_SCHEMA,
    })
    const ready = ((prep && prep.prepared) || []).filter((p) => p && p.ready)
    if (ready.length === 0) {
      log(`Vague ${wave} : aucun worktree pret — arret.`)
      break
    }
    const waveBaseSha = (prep && prep.wave_base_sha) || boot.tip_sha

    // ---- Implement (/tdd) + Review, par issue, en parallele (worktrees pre-crees) ----
    // pipeline : implement PUIS review pour la MEME issue, sans barriere entre issues.
    const results = await pipeline(
      ready,
      (prevResult, p) => agent(implPrompt(p, integration_branch), {
        label: `impl·#${p.number}`, phase: `Vague ${wave} · Implement+Review`, schema: IMPL_SCHEMA,
      }).then((r) => ({ ...p, ...(r || {}), committed: !!(r && r.committed) })),
      (impl) => {
        if (!impl || !impl.committed) return impl   // rien a relire
        return agent(reviewPrompt(impl, integration_branch), {
          label: `review·#${impl.number}`, phase: `Vague ${wave} · Implement+Review`, schema: REVIEW_SCHEMA,
        }).then((rev) => ({ ...impl, review: rev || null }))
      },
    )

    const completed = results.filter((r) => r && r.committed)
    log(`Vague ${wave} : ${completed.length}/${ready.length} issue(s) avec commits.`)
    if (completed.length === 0) {
      log(`Vague ${wave} : aucun commit — arret (rien a merger).`)
      waves.push({ wave, planned: planned.length, completed: 0, merge: null, test: null })
      break
    }
    // Ordre de merge deterministe : numero d'issue croissant.
    completed.sort((a, b) => a.number - b.number)

    // ---- Merge (dans la branche integration) ----
    phase(`Vague ${wave} · Merge`)
    let merge = await agent(mergePrompt(completed, integration_branch, integration_worktree), {
      label: `merger·v${wave}`, phase: `Vague ${wave} · Merge`, schema: MERGE_SCHEMA,
    })
    if (merge && merge.build_ok === false) runOk = false
    // Seules les branches CONFIRMEES mergees sont supprimables au cleanup.
    if (merge && Array.isArray(merge.merged_branches)) allMergedBranches.push(...merge.merged_branches)
    // Les issues effectivement traitees ne seront pas re-proposees aux vagues suivantes.
    handled.push(...completed.map((c) => c.number))

    // ---- Test couche 5 (best-effort) + fix-loop borne ----
    let test = null
    if (!SKIP_TEST) {
      phase(`Vague ${wave} · Test L5`)
      test = await agent(testPrompt(waveBaseSha, merge || {}, completed, integration_worktree), {
        label: `tester-L5·v${wave}`, phase: `Vague ${wave} · Test L5`, schema: TEST_SCHEMA,
      })

      // Sur FAIL : tenter jusqu'a L5_FIX_TRIES corrections du CODE PRODUIT (jamais le scenario),
      // re-tester apres chaque fix. Fallback garanti : si toujours rouge apres N (ou fix non
      // aboutie / hors-perimetre), on retombe sur le comportement historique (surface au gate humain).
      let fixTry = 0
      while (
        test && test.verdict === 'FAIL' &&
        fixTry < L5_FIX_TRIES &&
        (!budget.total || budget.remaining() > WAVE_FLOOR)
      ) {
        fixTry++
        const fp = `Vague ${wave} · Fix L5 ${fixTry}/${L5_FIX_TRIES}`
        phase(fp)
        const fix = await agent(l5FixPrompt(test, integration_branch, integration_worktree, fixTry, L5_FIX_TRIES), {
          label: `fix-L5·v${wave}·${fixTry}`, phase: fp, schema: FIX_SCHEMA,
        })
        if (!fix || !fix.committed || fix.kind !== 'fixed') {
          log(`Vague ${wave} : fix L5 ${fixTry}/${L5_FIX_TRIES} non aboutie (${(fix && fix.kind) || 'pas de fix'}) — arret du fix-loop, surface au gate.`)
          break
        }
        if (fix.integration_head_sha) merge = { ...(merge || {}), integration_head_sha: fix.integration_head_sha }
        log(`Vague ${wave} : fix L5 ${fixTry}/${L5_FIX_TRIES} appliquee (${(fix.touched || []).length} fichier(s)) — re-test.`)
        const rp = `Vague ${wave} · Re-test L5 ${fixTry}/${L5_FIX_TRIES}`
        phase(rp)
        test = await agent(testPrompt(waveBaseSha, merge || {}, completed, integration_worktree), {
          label: `tester-L5·v${wave}·retry${fixTry}`, phase: rp, schema: TEST_SCHEMA,
        })
      }

      if (test && test.verdict === 'FAIL') {
        runOk = false
        log(`Vague ${wave} : couche 5 = FAIL` + (fixTry ? ` apres ${fixTry} fix(s)` : '') + ` — regression.` + (STOP_ON_FAIL ? ' Arret des vagues.' : ' (poursuite, stop_on_fail=false)'))
        if (STOP_ON_FAIL) stop = true
      } else {
        log(`Vague ${wave} : couche 5 = ${(test && test.verdict) || 'aucun'}` + (fixTry ? ` (vert apres ${fixTry} fix L5).` : '.'))
      }
    }

    // ---- Promote (vague VERTE -> avance main en FF-only) ----
    // Verte = build OK apres merge ET (tests sautes OU couche 5 non-FAIL). On ne
    // promeut jamais une vague rouge : main ne contient que du vert. FF-only +
    // garde-fous : jamais de fusion divergente, jamais de push (cf promotePrompt).
    let promote = null
    const waveGreen = merge && merge.build_ok !== false && (SKIP_TEST || (test && test.verdict !== 'FAIL'))
    if (PROMOTE && waveGreen) {
      phase(`Vague ${wave} · Promote`)
      promote = await agent(
        promotePrompt(integration_branch, (merge && merge.integration_head_sha) || 'HEAD', PROMOTE_INTO),
        { label: `promote·v${wave}`, phase: `Vague ${wave} · Promote`, schema: PROMOTE_SCHEMA },
      )
      if (promote && promote.advanced) {
        log(`Vague ${wave} : ${PROMOTE_INTO} avance ${(promote.target_old_sha || '').slice(0, 9)} -> ${(promote.target_new_sha || '').slice(0, 9)} (${promote.method}).`)
      } else {
        log(`Vague ${wave} : ${PROMOTE_INTO} NON avance (${(promote && promote.method) || '?'})` + ((promote && promote.manual_cmd) ? ` — FF manuel : ${promote.manual_cmd}` : '') + '.')
      }
    } else if (PROMOTE && !waveGreen) {
      log(`Vague ${wave} : vague non verte -> ${PROMOTE_INTO} non avance (travail garde sur ${integration_branch}).`)
    }

    waves.push({
      wave,
      planned: planned.length,
      completed: completed.length,
      issues: completed.map((c) => ({ number: c.number, title: c.title, branch: c.branch })),
      merge, test, promote,
    })
  }
} finally {
  // ---- Cleanup garanti (seul proprietaire des worktrees) ----
  phase('Cleanup')
  if (boot && boot.run_tag) {
    await agent(
      cleanupPrompt(boot.run_tag, boot.integration_branch, boot.integration_worktree, runOk, allMergedBranches),
      { label: 'cleanup', phase: 'Cleanup', schema: CLEANUP_SCHEMA },
    )
  } else {
    log('Pas de bootstrap reussi — rien a nettoyer.')
  }
}

// Resume de promotion (la derniere vague verte qui a avance la branche promue donne le point de revert)
const promotedWaves = waves.filter((w) => w.promote && w.promote.advanced)
const lastPromote = promotedWaves.length ? promotedWaves[promotedWaves.length - 1].promote : null

return {
  ok: runOk,
  run_tag: boot && boot.run_tag,
  integration_branch: boot && boot.integration_branch,
  integration_reused: boot && boot.reused,
  source_branch: boot && boot.source_branch,
  promote_into: PROMOTE ? PROMOTE_INTO : null,
  promoted_waves: promotedWaves.length,
  // point de revert = SHA de la branche promue AVANT la 1re promotion de ce run
  revert_point: promotedWaves.length ? promotedWaves[0].promote.target_old_sha : null,
  promoted_head: lastPromote ? lastPromote.target_new_sha : null,
  waves_run: waves.length,
  waves,
  note:
    `Checkout utilisateur jamais modifie (hors FF de \`${PROMOTE_INTO}\` si c'est la branche checkout et propre). ` +
    `Consolidation sur la branche FIXE \`${TARGET}\` (reutilisee a travers les runs). ` +
    (PROMOTE
      ? `Vagues vertes promues en FF vers \`${PROMOTE_INTO}\`${promotedWaves.length ? ` (revert : git reset --hard ${(promotedWaves[0].promote.target_old_sha || '').slice(0, 9)})` : ''}. `
      : `Promote desactive : tout reste sur \`${TARGET}\` pour relecture/merge humain. `) +
    (SKIP_TEST ? 'Couche 5 sautee.' : 'Couche 5 best-effort par vague.'),
}
