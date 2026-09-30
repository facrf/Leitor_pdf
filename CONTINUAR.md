# Checkpoint — 2026-09-10

## Retomar

Peça: **Leia AGENTS.md e CONTINUAR.md, confira o diff e continue.**
Trabalhe apenas dentro deste projeto. Preserve as alterações existentes; não publique nem faça commit sem instrução.

## Estado atual

A revisão, correções e validação acumuladas chegaram a um ponto verificado. Não há mais bloqueio por ausência de Rust/Node: ferramentas foram instaladas exclusivamente em .cache/, ignorada pelo Git. Nenhum commit ou deploy foi realizado.

## Implementado

- Progresso: debounce, captura do livro/posição e fila serial de gravações; fechamento aguarda a fila; tentativa keepalive ao sair/ocultar. Backend valida objeto JSON e percentual.
- Streaming: HTML e outros originais ativos recebem attachment/CSP; nosniff para todos. Capas validam nome e confinamento canônico.
- Restauração: gate RwLock entre handlers HTTP e reserva de scan/capas; assets anteriores guardados e rollback por Drop; cópia transacional via rusqlite backup mantendo conexão.
- Backup: valida integridade, schema reconhecido, referências, caminhos e dados JSON; preserva library_root e network_metadata_enabled atuais. Pacote extraído é limpo por Drop, com propriedade acompanhando o worker mesmo após cancelamento.
- Rede: capas/OPDS validam URL, DNS e redirects, fixam IPs e desativam proxy de ambiente; corpos limitados durante a transferência. Provedores manuais preservam suporte deliberado a endpoints locais.
- Upload: PendingFile remove arquivos incompletos criados pela própria operação; corrigida limpeza OPDS que podia apagar destino de outra requisição.
- Scanner: metadata_scanned permite enriquecer após perfil econômico; metadata_edited protege alterações manuais/externas. Migração preserva títulos/autores legados por precaução.
- README, docs/api.md, docs/architecture.md e AGENTS.md atualizados.
- tests/with-local-tools.sh configura Rust, Node e caches locais. test:ui inclui teste real da fila de progresso.

## Validação realmente concluída nesta sessão

Executada com ferramentas locais:
- cargo test --locked: **19 passaram, 0 falharam, 1 ignorado** (teste de volume opt-in de 2.287 arquivos).
- npm run test:api: **passou**, incluindo backup/restauração e progresso.
- npm run test:auth: **passou**.
- npm run test:browser: **1 cenário passou**, cobrindo catálogo, filtros, coleção, leitura, anotação, backup e retomada em 37% após reload.
- npm run test:ui: **passou**, incluindo contrato visual, sintaxe e fila de gravações.
- git diff --check: **passou**.

Assinaturas/lifetimes de rusqlite backup e reqwest resolve_to_addrs agora foram confirmados pelo compilador. Não repetir alegação antiga de cargo ausente.

## Comandos para repetir quando necessário

Execute a partir da raiz:
```bash
bash tests/with-local-tools.sh cargo test --locked
bash tests/with-local-tools.sh npm run test:ui
bash tests/with-local-tools.sh npm run test:api
bash tests/with-local-tools.sh npm run test:auth
bash tests/with-local-tools.sh npm run test:browser
```

Rust 1.98.1 em .cache/rust, Node 22.16.0 em .cache/node, Chromium/Playwright em .cache/playwright; caches temporários também locais. CARGO_TARGET_DIR aponta para .cache/rust/target, pois target/debug existente recusou escrita. Não alterar permissões de target. O wrapper não instala ferramentas.

## Limites conhecidos / trabalhos futuros

- PDF acompanha controles da aplicação, não a navegação interna do visualizador nativo. Texto/MOBI usa percentual manual; EPUB salva capítulo e CBZ página.
- Sem fila persistente offline nem coordenação de progresso entre abas. keepalive não garante entrega após crash.
- SQLite e pastas de assets não têm transação única durável: queda do processo pode exigir recuperação pelo backup preventivo. Drop registra falhas de cleanup; não protege contra kill/crash.
- Renomear pastas de assets exige escrita no pai; mounts individuais podem impedir restauração e gerar rollback.
- Validação de backup não é auditoria exaustiva de todas as constraints. Schemas personalizados com índices extras são recusados.
- SSRF tem testes de classificação de IP e limites HTTP locais, mas não teste completo de DNS rebinding/redirects; não declarar segurança exaustiva.
- O limite ZIP usa tamanho declarado das entradas; avaliar limite real de bytes extraídos em endurecimento futuro.
- Range não satisfazível continua ignorado com 200; isso não foi tratado como correção obrigatória sem analisar requisitos HTTP.
- A suite de volume ignorada não foi executada; desempenho de acervos grandes não foi medido.

## Próxima ação

Se o usuário apenas pedir status, informar que compilação e suites acima passaram. Se pedir continuar melhorias, escolher uma pendência concreta acima e adicionar regressão correspondente. Se pedir entregar/versionar, revisar diff e escopo antes de agir. Não recomeçar toda a revisão nem instalar novamente as ferramentas locais.
