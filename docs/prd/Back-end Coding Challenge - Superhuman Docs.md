# Back-end Coding Challenge

## Introdução

Olá! A empresa Bipa está em busca de pessoas talentosas para se juntarem à nossa equipe. Este desafio é uma oportunidade para mostrar seu conhecimento em desenvolvimento de servidores e sua capacidade de pensar fora da caixa. Estamos ansiosos para ver o que você pode criar!

## O Desafio

Queremos que você desenvolva um servidor que seja capaz de servir dados de nós da rede lightning para um aplicativo.

Não se preocupe se você não tem conhecimento sobre a rede lightning, isso não é relevante para o desafio.

O servidor deverá usar uma API externa (a ser descrita posteriormente) para coletar os dados sobre os nós e salvar em algum banco de dados.

Além disso, o servidor deverá expor esses dados através de uma API JSON (que poderia ser usada por um aplicativo, por exemplo).

Por fim, o projeto deve incluir instruções de como executá-lo, além de quaisquer dados necessários para seguir as instruções.

O projeto deve ser escrito na linguagem de programação **Rust**. Bibliotecas, bancos de dados e outras tecnologias podem ser escolhidas à vontade.

## Endpoint para importação dos dados

O endpoint abaixo é o que você deve usar para realizar as requisições:

```bash
curl -sSL "https://mempool.space/api/v1/lightning/nodes/rankings/connectivity"
```

### Exemplo de resposta

```json
[
  {
    "publicKey": "03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f",
    "alias": "ACINQ",
    "channels": 2908,
    "capacity": 36010516297,
    "firstSeen": 1522941222,
    "updatedAt": 1661274935,
    "city": null,
    "country": {
      "de": "Vereinigte Staaten",
      "en": "United States",
      "es": "Estados Unidos",
      "fr": "États Unis",
      "ja": "アメリカ  ",
      "pt-BR": "EUA",
      "ru": "США",
      "zh-CN": "美国 "
    }
  },
  {
    "publicKey": "035e4ff418fc8b5554c5d9eea66396c227bd429a3251c8cbc711002ba215bfc226",
    "alias": "WalletOfSatoshi.com",
    "channels": 2772,
    "capacity": 15464503162,
    "firstSeen": 1601429940,
    "updatedAt": 1661812116,
    "city": {
      "de": "Vancouver",
      "en": "Vancouver",
      "es": "Vancouver",
      "fr": "Vancouver",
      "ja": " バンクーバー市    ",
      "pt-BR": "Vancôver",
      "ru": "Ванкувер"
    },
    "country": {
      "de": "Kanada",
      "en": "Canada",
      "es": "Canadá",
      "fr": "Canada",
      "ja": " カナダ ",
      "pt-BR": "Canadá",
      "ru": "Канада",
      "zh-CN": " 加拿大 "
    }
  },
  ...
]
```

## Endpoint para exposição das informações

O servidor deve responder a uma requisição HTTP `GET /nodes` e responder os seguintes dados de cada nó:

- **`public_key`** — Incluindo o dado exatamente como vier da importação.
- **`alias`** — Incluindo o dado exatamente como vier da importação.
- **`capacity`** — Incluindo o valor em Bitcoin convertido do dado importado.
- **`first_seen`** — Incluindo o valor formatado em texto do dado importado.

### Exemplo de resposta

```http
HTTP GET /nodes
```

```json
[
  {
    "public_key": "03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f",
    "alias": "ACINQ",
    "capacity": "360.10516297",
    "first_seen": "2018-04-05T15:13:42Z"
  },
  {
    "public_key": "035e4ff418fc8b5554c5d9eea66396c227bd429a3251c8cbc711002ba215bfc226",
    "alias": "WalletOfSatoshi.com",
    "capacity": "154.64503162",
    "first_seen": "2020-09-30T01:39:00Z"
  },
  ...
]
```

O servidor deve servir os dados lendo de um banco de dados, de forma que a leitura dos dados da API externa seja feita fora do processamento dessa requisição.

Em outras palavras, o servidor deve ter uma sub-rotina de importação dos dados que executa periodicamente, pois para responder a requisição ele deverá apenas ler do seu banco de dados.

## Recomendações Gerais

- Fique à vontade para consultar a internet para desenvolver sua solução, mas é vetado o uso de ferramentas de IA (ChatGPT, CoPilot, etc.) no desafio.
- O código deverá ser comentado onde necessário para o entendimento do revisor.
- Você pode usar bibliotecas de terceiros se desejar.

## Requisitos

- O servidor **não deve crashar em nenhuma circunstância**, então certifique-se de tratar os erros adequadamente.
- O campo `capacity` representa a quantidade de Bitcoin que o node possui nos canais em **sats**. Desejamos que você faça a conversão de sats para Bitcoin, observando que **1 Bitcoin = 100.000.000 sats**. Ou seja, se a capacity do node for 550.000 sats, você deve mostrar `0,00550000 BTC`.
- O campo `firstSeen` é representado em **unix time**; é esperado que você faça a formatação para um formato de data e hora legível.
- O trabalho deve ser dividido em **commits lógicos** que permitam nossa avaliação do progresso da implementação. É recomendado que os commits não sejam manipulados, de forma que os horários reflitam os horários reais em que os commits foram criados.
- As mensagens de commit e comentários de código devem ser em **inglês**.
- Escreva um **README** (de preferência, em inglês) explicando brevemente sobre o projeto, quais tecnologias você usou, como executar o app e os testes (se possuir). Abaixo um template de README que gostaríamos que você entregasse junto do desafio:

```markdown
## Build tools & versions used

## Steps to run the app

## What was the reason for your focus? What problems were you trying to solve?

## How long did you spend on this project?

## Did you make any trade-offs for this project? What would you have done differently with more time?

## What do you think is the weakest part of your project?

## Is there any other information you'd like us to know?
```

## Entrega e avaliação

Se você não conseguir cumprir algum dos requisitos, fique à vontade para nos enviar mesmo assim. Após o término, coloque seu projeto em um repositório público no GitHub ou GitLab e envie o link para gente. Ou, se preferir, envie para gente em um arquivo compactado por e-mail.

A Bipa irá avaliar seu conhecimento técnico, a arquitetura da solução, a organização do seu código e a qualidade do app criado.

Esperamos que você dê o seu melhor e consiga demonstrar todo seu conhecimento. Em caso de dúvida, não hesite em nos contatar.
