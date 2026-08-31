# Operação, desempenho e recuperação

Este guia cobre a rotina recomendada para uma biblioteca grande, como 150 GB distribuídos em aproximadamente 2.287 livros.

## Primeira catalogação

1. Monte a pasta de livros em `/pdf` e o diretório persistente em `/data`.
2. Abra **Configurações**, confirme a raiz e selecione **Econômico**.
3. Clique em **Atualizar** e acompanhe descoberta, quantidade, arquivo atual e porcentagem.
4. Ao terminar, confira **Configurações → Saúde e manutenção**.
5. Se quiser extrair metadados embutidos de PDF/EPUB, use **Equilibrado** e altere a data dos arquivos que deseja reprocessar. Itens inalterados são intencionalmente reaproveitados.

A tarefa é sequencial. O perfil econômico intercala no mínimo 12 ms de pausa por item e busca aproximadamente 35% de ciclo ativo do worker; o equilibrado busca 65%. Esses valores não são limites rígidos do sistema operacional. Para uma cota forte no Docker:

```bash
docker update --cpus 0.50 estante-livre
```

As atualizações posteriores são incrementais: se caminho, tamanho e data de modificação em nanossegundos coincidirem, o aplicativo conserva metadados editados e evita reler o conteúdo. O resumo final separa novos, atualizados e inalterados.

## Títulos ilegíveis

Metadados embutidos passam por decodificação UTF-8, UTF-16 BE/LE ou Windows-1252. Caracteres de substituição (`�`), controles, textos excessivamente longos ou com alta densidade de símbolos são rejeitados. Nesse caso, o título passa a ser o nome do arquivo sem a extensão. Uma nova varredura também reconhece e corrige títulos corrompidos gravados por versões anteriores.

## Capas de PDF

O container inclui `pdftoppm` (Poppler). **Gerar capas pendentes** renderiza apenas a primeira página, uma por vez, com `nice -n 15`, largura/altura máxima de 900 px e intervalo de 250 ms. O mesmo processo pode ocorrer depois da varredura quando **Gerar capas locais de PDFs** está ativo.

Uma falha individual aparece como aviso e não interrompe as demais. PDFs cifrados, corrompidos ou sem primeira página continuam usando a capa tipográfica gerada pela interface.

## Duplicatas e exclusão

A varredura calcula SHA-256 sobre tamanho, primeiros 64 KiB e últimos 64 KiB. Isso evita ler 150 GB apenas para encontrar candidatos. Grupos iguais são **prováveis duplicatas**, não uma prova byte a byte para o miolo de arquivos grandes.

Antes de excluir:

- abra ou compare os caminhos exibidos;
- preserve pelo menos um arquivo;
- mantenha backup dos livros fora do aplicativo;
- digite exatamente o nome solicitado na confirmação.

A exclusão remove o original e apenas marca a entrada como indisponível. Não existe lixeira interna e o backup administrativo não contém `/pdf`.

## Backup e restauração

**Criar backup agora** executa um snapshot consistente do SQLite (`VACUUM INTO`) e cria um ZIP em `BACKUP_DIR`. O pacote contém:

- `library.db` com catálogo, configurações, coleções, progresso, notas e links;
- `covers/`;
- `branding/`;
- `manifest.json` com versão, autoria e data.

Os livros originais não são incluídos. Copie periodicamente tanto o ZIP quanto a pasta de livros para outro disco.

Ao restaurar, a interface exige `RESTAURAR`, valida travessia de diretórios, quantidade de entradas, tamanho descompactado, integridade SQLite e presença da tabela de livros. Antes da substituição é criado automaticamente outro backup de segurança. Aguarde a varredura/capas terminarem antes de restaurar.

ZIPs inválidos são recusados como erro de entrada (`HTTP 400`), e os diretórios temporários da tentativa são removidos. O E2E também confirma que um pacote corrompido não altera o catálogo existente.

## OPDS

`/api/opds` é um feed Atom/OPDS de aquisição com links para arquivos e capas. Pode receber os mesmos filtros de `/api/books`. Se a API estiver protegida, o cliente OPDS precisa oferecer HTTP Basic.

A importação é sempre manual, exige a opção de rede habilitada e uma confirmação. O backend aceita somente HTTP/HTTPS público, recusa localhost/redes privadas, limita o feed a 5 MiB, processa até 100 entradas e limita cada livro a 1 GiB. Depois do download, a interface inicia uma varredura.

## Autenticação

Defina ambos os valores no `.env` ou no Portainer:

```dotenv
AUTH_USERNAME=facrf
AUTH_PASSWORD=troque-por-uma-senha-longa
```

Recrie o container. As credenciais ficam somente no ambiente e não entram no SQLite nem nos backups. `/api/health` e `/api/public/{token}/file` permanecem acessíveis sem login por finalidade. Use HTTPS em um proxy reverso ou uma VPN; HTTP Basic sem TLS apenas codifica, mas não cifra, as credenciais.

Há exemplos completos para [Caddy e Traefik](https.md).

## Verificações

O ciclo curto usado pelo build da imagem é:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm ci
npm run test:ui
npm run test:api
npm run test:auth
npx playwright install chromium
npm run test:browser
docker compose config --quiet
docker build --tag estante-livre:local .
```

O teste de volume cria 2.287 arquivos pequenos em `tests/runtime-data-scanner-*`, verifica progresso monotônico e remove a fixture ao final:

```bash
cargo test indexes_2287_files -- --ignored
```

Para uma validação realista, use uma cópia do acervo, confira a carga com `docker stats estante-livre` e teste a restauração do pacote antes de depender dele.

## Checklist de publicação e Portainer

1. Confirme que a árvore Git está limpa e que o workflow **Publicar imagem Docker** concluiu os jobs de testes e GHCR.
2. Confirme no GHCR que `latest` aponta para um manifesto com `linux/amd64`, `linux/arm64` e `linux/arm/v7`.
3. No Portainer, use **Pull and redeploy** para baixar o novo digest, sem remover os volumes.
4. Confira nos logs a mensagem `Estante Livre pronta` e abra `/api/health`.
5. Execute uma varredura curta, abra um livro, salve uma anotação e crie um backup.
6. Faça download do backup, restaure-o em uma instalação descartável e confira catálogo e progresso.
7. Se houver acesso remoto, valide HTTPS conforme [o guia dedicado](https.md) e confirme que a porta `20000` não está pública.
8. Mantenha a versão anterior da imagem ou seu digest anotado para rollback; o banco deve continuar acompanhado por backups próprios.
