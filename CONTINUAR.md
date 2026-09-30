# Checkpoint — 2026-09-30

## Retomar

Leia AGENTS.md, confira git status/git diff e este checkpoint antes de editar.
Trabalhe apenas neste projeto. O usuário autorizou commit e push em 2026-09-30.
Nenhum deploy manual foi realizado.

## Estado atual

Concluída a implementação dos cinco problemas e das três melhorias autorizadas
na revisão de 2026-09-30. Alterações preparadas para commit e push em main;
confira git status e o remoto ao retomar para confirmar o envio.

## Implementado nesta revisão

- Posições de notas, progresso e restauração usam models::validate_location:
  objeto de até 8 KiB, campos de página/capítulo inteiros seguros, percentual 0–100.
  Nota sem posição recebe {}; restauração conserva compatibilidade com null legado.
- Rótulos de anotações são escapados antes de inserir HTML, inclusive para dados legados.
- Hrefs EPUB codificados são decodificados antes de procurar as entradas ZIP;
  escapes inválidos e travessia acima da raiz são recusados.
- CSP de leitores permite CSS inline/local no sandbox, conservando bloqueio de scripts/conexões.
- Aplicar metadados prepara a capa antes da gravação. Metadados e referência de capa
  são atualizados juntos; PendingFile limpa arquivos novos em falha/cancelamento.
  Nomes únicos evitam sobrescrever capas existentes. Imagens antigas são conservadas.
- Catálogo e OPDS usam limit (padrão 60, máximo 200) e offset (padrão 0), com total
  filtrado e desempate por ID. Interface navega entre páginas e descarta respostas antigas.
  OPDS fornece link next conservando filtros. Clientes precisam percorrer páginas.
- Renderização PDF usa COVER_TIMEOUT_SECONDS (padrão 60, permitido 1–600),
  encerra/aguarda processo no timeout e permite continuar com o próximo arquivo.
- Inicialização recusa autenticação parcial e usuário com dois-pontos.
- Regressões Rust em tests/rust/, incluídas pelos módulos internos e copiadas pelo Dockerfile.
  Suites API/navegador/autenticação ampliadas; documentação e ambiente atualizados.

## Verificações realmente executadas

Com bash tests/with-local-tools.sh:
- cargo check --locked: passou durante a implementação.
- cargo test --locked: 28 passaram, 0 falharam, 1 teste de volume ignorado.
- npm run test:ui: passou (sintaxe JS, contrato visual e fila de progresso).
- npm run test:api: passou, incluindo paginação, posições inválidas, falhas de metadados
  sem alteração do livro e backup/restauração de nota com posição padrão.
- npm run test:auth: passou, incluindo recusa de configuração parcial na inicialização.
- npm run test:browser: 1 cenário passou, incluindo páginas, filtros, estilos computados
  do leitor e escape de HTML em posição legada.
- git diff --check: passou.
- cargo fmt não pôde executar: rustfmt não está instalado na ferramenta local.
  Nenhuma ferramenta foi instalada nesta revisão.
- A imagem Docker não foi construída nesta revisão.

## Ferramentas locais

Use tests/with-local-tools.sh para Rust, Node, Playwright e temporários dentro de .cache/.
O wrapper não instala ferramentas. Não alterar permissões de target/.

## Limites preservados

- PDF acompanha controles da aplicação, não a navegação interna do visualizador nativo.
  Texto/MOBI usa percentual manual; EPUB salva capítulo e CBZ salva página.
- Sem fila persistente offline nem coordenação entre abas; keepalive não garante entrega após crash.
- Não há transação durável única entre SQLite e imagens nem recuperação conjunta automática
  após crash. Pastas anteriores são preservadas até commit; mounts podem impedir renomeação.
- Validação de backup não é auditoria de todas as constraints; schemas com índices extras são recusados.
  Backups antigos com posições inválidas (exceto null legado de notas) continuam recusados.
- SSRF possui validação DNS/IP/redirects, mas não testes completos de DNS rebinding.
- Limites ZIP ainda usam tamanhos declarados. Range não satisfazível ainda pode retornar 200.
- O teste de volume ignorado não foi executado; desempenho em acervo real não foi medido.
- Paginação por offset pode mudar se o catálogo for alterado entre páginas.
- Imagens antigas não têm coleta automática.

## Próximo passo

Não há pendência na implementação autorizada. Commit e push foram autorizados.
Para publicação de imagem ou deploy manual, confira as instruções do usuário e
valide a construção da imagem antes de prosseguir.
