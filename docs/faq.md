# FAQ — Lightning Nodes Service

Respostas para as perguntas mais prováveis sobre o projeto: decisões de
arquitetura, contrato da API, persistência, tratamento de falhas, testes,
trade-offs e um glossário das bibliotecas e dependências.

Documentos relacionados:

- [`../README.md`](../README.md) — instruções de execução e configuração.
- [`api.md`](api.md) — contrato HTTP.
- [`prd/Architecture.md`](prd/Architecture.md) — decisões de arquitetura.
- [`prd/tech-spec/01-database.md`](prd/tech-spec/01-database.md) — banco e migrations.
- [`prd/tech-spec/02-api.md`](prd/tech-spec/02-api.md) — rotas, middleware e erros.
- [`prd/tech-spec/03-logging.md`](prd/tech-spec/03-logging.md) — logging e auditoria.

---

## Arquitetura

### 1. Por que essa arquitetura em camadas? Não é exagero para um desafio?

A separação é intencional, mas pequena: dois casos de uso (`import_nodes` e
`list_nodes`), três portas (`NodesGateway`, `NodesRepository` e `RunLogger`) e
implementações nas bordas. O domínio não conhece Axum, SQLx nem Reqwest, então
as regras de formatação e validação são testáveis sem banco ou rede. Não há um
arquivo por conceito nem abstração sem uso real.

### 2. Como está organizado o código?

- `domain`: value objects (`money::Sats`, `PublicKey`, `FirstSeen`), a entidade
  `Node` e as regras de formatação.
- `application`: casos de uso `import_nodes` e `list_nodes`, além do
  `NodePresenter`, que monta a representação da API.
- `gateways`: porta e implementação do mempool, além de mock.
- `repositories`: porta, implementações PostgreSQL/SQLite/in-memory e
  factories.
- `routes`: handlers, DTOs, erros HTTP, health check e rate limiting.
- `logging`: porta `RunLogger` e backend de arquivo.
- `composition` e `main`: montagem explícita das dependências.

### 3. Por que usar factories?

Para escolher a implementação em tempo de configuração (`PERSISTENCE`) e para
permitir mocks nos testes. A factory só constrói a implementação; ela não
implementa o comportamento da porta. Isso mantém a composição explícita e evita
acesso global.

### 4. Por que não usou `async-trait`?

A linguagem já suporta `async fn` em traits nativamente. Foram usados traits
genéricos com `impl Future + Send`, que evitam boxing e dispatch dinâmico. A
desvantagem é que eles não são dyn-compatible e a assinatura fica um pouco mais
verbosa; para este projeto, o ganho de simplicidade em runtime compensa.

---

## Dados e contrato da API

### 5. Como converte `capacity` de sats para BTC sem erro de arredondamento?

A conversão fica isolada no value object `Sats`, em `src/domain/money.rs`. A
representação canônica é o `Display`, que usa `u64` e aritmética inteira: divide
por `100_000_000` e obtém o resto com `%`, formatando o resto com exatamente
oito dígitos. Não há `f64`, então não há erro de ponto flutuante. Exemplos:
`36010516297` vira `"360.10516297"` e `550000` vira `"0.00550000"`. O método
`as_btc()` continua existindo como alias de compatibilidade.

`Sats` é um value object clássico: não tem identidade, é imutável e `Copy`,
compara por valor (`Eq`, `Ord`, `Hash`), tem a unidade como invariante e não
conhece banco, HTTP nem `serde`. Também oferece `ZERO`, `checked_add`,
`checked_sub` e `is_zero`, que serão úteis quando existirem transações.

A montagem da resposta HTTP passa pelo `NodePresenter`
(`src/application/present_nodes.rs`), que é a única porta de entrada da
representação da API. Handlers não chamam formatadores diretamente, então não
existe uma segunda regra de formatação que possa divergir.

Além da tabela de casos de borda, os testes fazem duas verificações fortes:

- **Round-trip:** formatam 10 mil valores pseudoaleatórios e convertem a string
  de volta para sats usando apenas inteiros; o valor precisa ser idêntico ao
  original, provando que nenhum dígito foi perdido.
- **Limite do `f64`:** o caso `2^53 + 1` (primeiro inteiro que um `f64` não
  representa exatamente) produz `"90071992.54740993"`, e `u64::MAX` produz
  `"184467440737.09551615"`. Uma implementação com ponto flutuante falharia
  nesses valores.

### 6. Por que `capacity` é string e não número?

Porque o contrato pede exatamente oito casas decimais e precisa preservar a
precisão. Se fosse número JSON, a representação poderia variar e, em
JavaScript, valores grandes poderiam perder precisão. A string também deixa
explícito que o campo é formatado, não um valor bruto.

### 7. Como formata `first_seen`?

O valor vem em Unix time (segundos). É convertido para `DateTime<Utc>` com
Chrono e serializado em RFC 3339 com segundos e sufixo `Z`, por exemplo
`1522941222` → `"2018-04-05T15:13:42Z"`. O fuso é sempre UTC, o que evita
ambiguidade.

### 8. Por que guardar sats no banco e não BTC?

Porque sats é a unidade inteira e exata. A conversão para BTC é uma
responsabilidade de apresentação, feita na borda HTTP. Guardar o valor bruto
também preserva o dado importado e facilita uma futura mudança de formato sem
migração.

### 9. Por que guardar `rank_position`?

A API do mempool devolve os nós em ordem de conectividade, e essa ordem é útil
para o consumidor. A posição é persistida e a consulta usa
`ORDER BY rank_position ASC`, garantindo que a resposta mantenha o ranking
mesmo depois de passar pelo banco. Não é um requisito explícito, mas evita
ordenação arbitrária.

### 10. O que acontece com nós inválidos ou duplicados?

Cada item é validado: `publicKey` não pode ser vazio, `capacity` deve existir e
ser não negativa, `firstSeen` deve ser um timestamp válido. Itens inválidos são
descartados e registrados em log; chaves duplicadas também são ignoradas. Se
sobrar pelo menos um nó válido, o snapshot é importado com os válidos. Se todos
forem inválidos, o import é tratado como vazio e o snapshot anterior é mantido.

### 11. `public_key` e `alias` são realmente “exatamente como vieram”?

Sim. A única validação da chave é não ser vazia; não há trim, lowercase nem
decodificação hexadecimal. O alias é armazenado como string; se vier ausente,
vira string vazia para respeitar o contrato de resposta. Campos desconhecidos no
JSON são ignorados pelo Serde, o que dá tolerância a evoluções da API.

### 12. O que acontece se a API mudar o schema?

O envelope é `Vec<RawNode>` com campos opcionais. Campos novos são ignorados;
campos obrigatórios ausentes fazem o nó ser descartado. Se o corpo não for JSON
válido ou o status não for 2xx, o import falha por inteiro e o snapshot anterior
é preservado. O próximo passo seria ter testes de contrato mais rígidos contra
o payload real.

### 13. Por que a resposta não devolve todos os campos do mempool?

O contrato pede apenas `public_key`, `alias`, `capacity` e `first_seen`. Expor
somente o necessário reduz o acoplamento com o formato externo e evita que
campos como `channels`, `updatedAt` ou `country` virem parte do contrato sem
necessidade. Se o consumidor precisar de mais dados, a extensão é aditiva.

### 14. Por que `public_key` é a chave primária da tabela?

Porque é o identificador natural e único de um nó Lightning. Usar a própria
chave evita um UUID intermediário, impede duplicatas no banco e simplifica a
substituição do snapshot. O parser também remove duplicatas antes de gravar.

### 15. Para que serve `imported_at` se a API não o usa?

É uma coluna de operação e auditoria: registra quando a linha entrou no
snapshot, útil para investigar recargas e problemas de importação. A API expõe
apenas os quatro campos do contrato; os metadados internos não vazam para a
resposta.

---

## Persistência e concorrência

### 16. Por que substituir o snapshot inteiro em vez de fazer upsert?

Porque o endpoint é um ranking: ele representa o conjunto atual, e substituir
tudo é simples e atômico. O `DELETE` e os `INSERT`s acontecem na mesma
transação, então leitores veem o snapshot antigo ou o novo, nunca um estado
intermediário. Com mais tempo, seria feito upsert com remoção dos ausentes,
inserção em lote e metadados de importação.

### 17. Um `GET /nodes` durante a importação pode ver dados misturados?

Não. PostgreSQL usa MVCC e SQLite está em modo WAL com a escrita dentro de
transação; a leitura enxerga apenas dados commitados. A troca é tudo-ou-nada. O
único caso de “mistura” seria se houvesse vários commits separados, o que não
acontece.

### 18. Por que PostgreSQL, SQLite e in-memory?

PostgreSQL é o backend principal e o padrão do Docker Compose. SQLite serve
para rodar localmente sem Docker e para testes de integração com banco real.
In-memory existe para demos e testes de fumaça, e deixa claro que não é
persistência de produção. O requisito de banco é atendido pelo PostgreSQL; os
outros são conveniências de desenvolvimento.

### 19. Por que `PERSISTENCE` é obrigatório?

Para deixar a escolha do backend explícita e evitar um padrão implícito no
processo. O Docker Compose fornece `PERSISTENCE=postgres` por padrão; fora do
Compose, o operador precisa declarar `POSTGRES`, `SQLITE` ou `IN_MEMORY`. A
validação falha rápido se o valor estiver ausente ou for desconhecido.

### 20. Como as migrations funcionam?

As migrations ficam em `migrations/` para PostgreSQL e `migrations-sqlite/`
para SQLite. O factory do repositório aplica as pendentes automaticamente na
inicialização; também existe o comando `migrate` para rodar de forma explícita.
Elas são forward-only e idempotentes pelo controle de versão do SQLx.

### 21. Por que não usou `sqlx::query!` com validação em tempo de compilação?

Porque isso exigiria um banco disponível no build ou metadados offline
commitados, complicando o Docker. A opção foi SQL em runtime, que mantém o
build simples. A troca é não ter validação de SQL em tempo de compilação; com
mais tempo, seria usado `query!` e o diretório `.sqlx`.

### 22. O que acontece se o banco estiver fora no boot?

O PostgreSQL tenta conectar até 10 vezes, com backoff exponencial de 500 ms até
5 segundos, e então encerra com erro se não conseguir. O Compose tem
`restart: unless-stopped` e `depends_on` com healthcheck do banco. Em runtime,
falha de leitura vira `500`, falha de health vira `503` e falha de import é
logada, preservando o snapshot anterior.

### 23. Duas instâncias da aplicação podem importar ao mesmo tempo?

Sim, e não há lock distribuído. Cada instância roda seu próprio scheduler, faz
seu próprio fetch e substitui o snapshot; a última escrita vence. Como o
snapshot é idempotente na prática, o resultado não corrompe, mas há trabalho
duplicado e uma janela de corrida. Para produção com múltiplas réplicas, o
caminho seria um lock consultivo no PostgreSQL ou um worker dedicado de
importação.

---

## Falhas, scheduler e operação

### 24. O requisito diz “não deve crashar em nenhuma circunstância”. Como isso é tratado?

O caminho de erros esperados não usa `panic`: tudo vira `Result`, os erros são
mapeados para `500`/`503`, o scheduler captura falhas de import e continua
rodando, e o desligamento é gracioso via Ctrl+C/SIGTERM. O processo só termina
de forma controlada se a configuração for inválida, se a porta não puder ser
aberta ou se o banco não conectar no boot. Existe um `expect` no mutex do rate
limiter que poderia ser removido para eliminar o último risco teórico de panic;
como não há panic enquanto o lock está tomado, ele não dispara na prática.

### 25. Como o scheduler funciona? Pode haver imports sobrepostos?

É usado `tokio::time::interval` com `MissedTickBehavior::Delay`. O primeiro
tick é imediato, então o serviço já popula o banco no startup. O loop espera o
import terminar antes de processar o próximo tick, então não há duas
importações concorrentes na mesma instância. Se uma importação demorar mais que
o intervalo, o próximo tick é atrasado em vez de acumular execuções.

### 26. Por que o intervalo padrão é 300 segundos?

O ranking de conectividade não muda a cada segundo, e consultar o mempool com
muita frequência é desnecessário. 300 segundos é um equilíbrio razoável; o
valor é configurável por `IMPORT_INTERVAL_SECS` e tem mínimo de 30 segundos
para evitar abuso.

### 27. O que acontece se o mempool retornar uma lista vazia?

O import é marcado como `SkippedEmpty` e o snapshot anterior é mantido. Isso
evita que uma falha silenciosa da API apague os dados bons. A mesma lógica vale
quando todos os itens são inválidos.

### 28. E se o mempool retornar apenas parte do ranking?

Aí está a principal fragilidade: o serviço confia que o endpoint devolve o
ranking completo. Cada nó é validado e uma lista vazia é rejeitada, mas não é
possível provar que a lista está completa. Uma resposta parcial, mas válida,
substituiria o snapshot por um subconjunto. Com mais tempo, seria possível
comparar contagem/tamanho esperado, guardar metadados da coleta e talvez fazer
merge com o snapshot anterior.

### 29. Como funciona o rate limiting?

É um contador de janela fixa por IP, aplicado apenas em `/nodes` com
`route_layer`. O padrão é 60 requisições por 60 segundos, configurável. Ao
exceder, retorna `429` com `Retry-After`. `/health` fica fora do limiter de
propósito, para monitoramento não ser bloqueado. Se o endereço do cliente não
estiver disponível, é usado um bucket compartilhado em vez de rejeitar a
requisição.

### 30. Rate limiting não estava no desafio. Por que adicionou?

Como endurecimento de produção: o endpoint é público e barato, mas um cliente
mal comportado poderia consumir recursos. Foi feito de forma isolada e
configurável, sem interferir no contrato. O risco é atrapalhar testes
automatizados que fazem muitas chamadas; por isso o limite é razoavelmente alto
e ajustável.

### 31. Como funciona o desligamento?

O `axum::serve` usa `with_graceful_shutdown`, escutando Ctrl+C e SIGTERM.
Quando o sinal chega, o servidor para de aceitar novas conexões, o canal
`watch` avisa o scheduler e o processo aguarda a task de importação terminar
antes de sair. Isso evita deixar uma transação pela metade.

### 32. Como é a observabilidade?

Há três camadas: logs de console com `tracing` e campos estruturados;
`/health`, que faz `ping` no repositório e retorna `200` ou `503`; e um log de
auditoria por execução de importação, em arquivo diário, com contagem total,
quantos nós são novos e o motivo de falha. O log de auditoria é separado do log
de diagnóstico porque o console é ruidoso e rotativo, enquanto o histórico de
importações precisa ser simples de ler.

### 33. Por que existe o comando `import` além do scheduler?

Para operação e depuração: permite popular o banco ou testar a integração sem
subir o servidor HTTP. Ele aplica migrations, busca uma vez, grava o snapshot,
registra a execução e sai com código `0` em sucesso/lista vazia ou não-zero em
falha. É útil em pipeline e para reproduzir problemas.

### 34. Preciso rodar algum comando para popular o banco?

Não. O primeiro import roda imediatamente após a inicialização. Se
`GET /nodes` retornar `[]`, basta aguardar alguns segundos e tentar novamente.
Para uma carga única sem subir o servidor, existe `cargo run -- import` (ou
`make import`).

---

## Testes e qualidade

### 35. Como rodar o projeto?

O caminho principal é `cp .env.example .env` e
`docker compose up --build -d`; depois
`curl http://localhost:3000/health` e `curl http://localhost:3000/nodes`. As
migrations rodam sozinhas. Também é possível rodar localmente com
`PERSISTENCE=postgres`, `sqlite` ou `in_memory` e `cargo run`.

### 36. Como rodar os testes?

`cargo test --all-targets` roda a suíte padrão; `cargo fmt --check` e
`cargo clippy --all-targets -- -D warnings` verificam estilo e lint. O
`Makefile` agrupa em `make test`, `make test-unit`, `make test-integration` e
`make test-live`. Os testes live e o e2e são `#[ignore]` por padrão para manter
a suíte offline e determinística.

### 37. O que está coberto pelos testes?

Testes unitários cobrem domínio (conversão de capacidade, timestamp, chave
vazia), configuração, política de importação, parsing do payload, rotas, rate
limiter e logging. Testes de integração usam `wiremock` para o gateway e bancos
SQLite temporários para o repositório. O e2e sobe um servidor HTTP real e o
teste live chama o mempool.space de verdade quando solicitado.

### 38. O que não está coberto?

Não há um teste de integração dedicado ao repositório PostgreSQL na suíte
padrão; o caminho Postgres é exercitado pelo e2e quando `PERSISTENCE=postgres`.
Também não há testes de carga, fuzzing nem teste de contrato contra o payload
real em CI. O e2e e o live são ignorados por padrão, então a suíte padrão não
prova a integração externa.

---

## Trade-offs e limitações

### 39. Qual é a maior fraqueza do projeto?

A confiança no snapshot externo. A substituição total é simples e atômica, mas
assume que o ranking veio completo. Uma resposta parcial e válida pode reduzir
o snapshot. Além disso, é uma única instância sem lock distribuído; duas
réplicas fariam importações concorrentes e a última escrita venceria.

### 40. O que faria diferente com mais tempo?

- Upsert com remoção dos ausentes e inserção em lote, em vez de `DELETE` +
  `INSERT` um a um.
- Lock consultivo no PostgreSQL ou um worker dedicado para múltiplas réplicas.
- `sqlx::query!` com metadados offline para validar SQL em compilação.
- Testes de contrato contra o payload real e alarme para queda brusca no
  número de nós.
- Paginação, cache/ETag e CORS se o consumo crescer.

### 41. Como escalaria para mais nós ou mais tráfego?

O dataset atual é pequeno, então a substituição total e a leitura sem paginação
são suficientes. Para crescer, seria necessário inserção em lote, upsert
incremental, índices adequados, paginação no `/nodes`, cache de leitura e, se
necessário, separar o importador em um worker. O caminho de leitura continuaria
sem chamar a API externa.

### 42. Por que não tem autenticação, paginação, CORS ou cache?

Porque o desafio pede uma API JSON somente leitura sobre um conjunto pequeno e
público. Adicionar isso sem necessidade aumentaria o escopo e o risco. CORS
faria sentido se um app web de outra origem consumisse a API; paginação, se a
lista crescesse; cache/ETag, se o volume de leitura justificasse.

### 43. Por que o container roda como non-root e tem healthcheck?

Como boa prática de deploy: o Dockerfile usa multi-stage build, instala apenas
o necessário no runtime, cria um usuário sem privilégios e expõe um
`HEALTHCHECK` que consulta `/health`. O Compose também verifica o PostgreSQL
antes de subir a aplicação e monta o diretório de logs.

### 44. O projeto está 100% pronto para entrega?

O núcleo funcional está: importa, persiste, serve os quatro campos corretamente
e tem testes passando. Antes da entrega, ainda vale revisar itens de
acabamento: preencher o tempo gasto no template do README, garantir que o
repositório esteja público e conferir se todas as variáveis documentadas estão
disponíveis no fluxo do Compose. Nada disso exige mudança de arquitetura.

---

## Glossário de bibliotecas e dependências

As dependências de produção ficam em `[dependencies]` no `Cargo.toml`; as
usadas apenas em testes ficam em `[dev-dependencies]`. As versões exatas estão
no `Cargo.toml` e no `README.md`.

### Runtime e HTTP

**`tokio`** — runtime assíncrono do Rust. É ele que executa as tarefas
assíncronas, o servidor HTTP e o scheduler. No projeto, fornece `#[tokio::main]`
e `#[tokio::test]`, `TcpListener`, `interval`, `watch` e os handlers de
`SIGTERM`/Ctrl+C. Não é um framework web; é a base sobre a qual o Axum roda.

**`axum`** — framework HTTP construído sobre Tokio/Hyper. Define o `Router`, as
rotas (`GET /nodes`, `GET /health`), os extractors (`State`, `Json`), o
middleware de rate limiting/logging e as respostas JSON. É a camada de entrada
HTTP.

**`reqwest`** — cliente HTTP assíncrono. É usado somente no
`MempoolNodesGateway` para buscar o ranking no mempool.space. As features
`json` e `rustls` habilitam desserialização JSON e TLS sem OpenSSL. O caminho de
`GET /nodes` nunca usa `reqwest`.

**`rustls`** — implementação de TLS em Rust usada pelo `reqwest`. Evita a
dependência de OpenSSL no container. O `aws-lc-sys`, backend criptográfico
usado transitivamente pelo rustls, é o motivo de o Dockerfile instalar `cmake`
no estágio de build.

### Persistência

**`sqlx`** — toolkit SQL assíncrono. Fornece pool de conexões, execução de
queries, `FromRow`, migrations e o macro `sqlx::migrate!`. As features habilitam
PostgreSQL, SQLite, integração com `chrono`, migrations e macros. O projeto usa
SQL explícito em runtime, não um ORM e não os macros `query!`/`query_as!` de
validação em tempo de compilação.

**PostgreSQL 17** — banco relacional principal. É o backend padrão do Docker
Compose, guarda o snapshot em `nodes` e oferece transações, `TIMESTAMPTZ` e
MVCC para que o `GET /nodes` nunca veja um snapshot pela metade.

**SQLite** — banco embutido em arquivo. Serve para desenvolvimento local sem
Docker e para testes de integração com banco real. Usa o mesmo `sqlx`, mas com
migrations próprias em `migrations-sqlite/`.

### Serialização e tempo

**`serde`** — framework de serialização/desserialização. Os derives `Serialize`
e `Deserialize` transformam `NodeResponse` em JSON e o payload do mempool em
`RawNode`. O `#[serde(rename_all = "camelCase")]` mapeia `publicKey` para
`public_key`, por exemplo.

**`serde_json`** — implementação de JSON usada pelo Serde. É usada nos testes
para comparar o corpo da resposta e pelo `reqwest` para ler o JSON da API
externa.

**`chrono`** — biblioteca de data e hora. Converte Unix time em
`DateTime<Utc>`, formata `first_seen` em RFC 3339 com sufixo `Z` e integra com o
`sqlx` para mapear `TIMESTAMPTZ`.

### Erros e logs

**`thiserror`** — derive para implementar `std::error::Error` e `Display` em
enums de erro. É usado em todos os erros tipados do projeto, como
`DomainError`, `GatewayError`, `ImportError`, `ListError`, `RepositoryError` e
`ConfigError`, evitando boilerplate manual. O projeto não usa `anyhow`; os
erros são tipados.

**`tracing`** — fachada de logs estruturados. Fornece macros como
`tracing::info!`, `warn!` e `error!`, usadas no scheduler, nas rotas, nos
repositórios e no gateway. Não é o log de auditoria: aquele é implementado em
`src/logging` e grava uma linha por importação.

**`tracing-subscriber`** — implementação que consome os eventos do `tracing` e
os imprime no console. A feature `env-filter` permite controlar o nível com
`RUST_LOG`.

### Testes (dev-dependencies)

**`wiremock`** — servidor HTTP falso para testes. Sobe um servidor local que
responde o payload do mempool, permitindo testar sucesso, status de erro, JSON
inválido e falha de conexão sem depender da internet.

**`tower`** — biblioteca de middleware sobre a qual o Axum é construído. Nos
testes, usamos `tower::ServiceExt::oneshot` para chamar o `Router` diretamente,
sem abrir uma porta de rede.

**`tempfile`** — cria diretórios e arquivos temporários. É usado nos testes de
SQLite para criar um banco descartável por teste, sem sujar o projeto.

### Ferramentas de execução (não são crates)

**Cargo e rustc** — build, dependências, testes e lint. `cargo test`,
`cargo fmt` e `cargo clippy` são os comandos principais.

**Docker e Docker Compose** — empacotam a aplicação em imagem multi-stage e
orquestram a aplicação com o PostgreSQL. O Compose também configura
healthchecks, volumes e o diretório de logs.

**Make** — atalhos para subir o stack, rodar migrations, importar uma vez e
executar as suítes de teste. O `Makefile` também carrega o `.env` para os testes
live, sem que o binário Rust leia `.env` sozinho.

### O que não usamos

- **`dotenv`**: o `.env` é carregado pelo Makefile ou pelo Compose; o processo
  Rust lê apenas variáveis de ambiente já exportadas.
- **ORM**: o `sqlx` é usado com SQL explícito.
- **`async-trait`**: os traits usam `async fn` nativo.
- **`mockall`**: os mocks de gateway/repositório são escritos à mão.
- **`anyhow`**: os erros são enums tipados com `thiserror`.
- **`proptest`**: o teste pseudoaleatório de `Sats` é determinístico e não
  precisa de dependência extra.

### Mapa mental

```text
Servidor HTTP ........ axum + tokio
Cliente HTTP ......... reqwest + rustls
Banco ................ sqlx + PostgreSQL/SQLite
JSON ................. serde + serde_json
Datas ................ chrono
Erros ................ thiserror
Logs ................. tracing + tracing-subscriber
Testes ............... wiremock + tower + tempfile
```

