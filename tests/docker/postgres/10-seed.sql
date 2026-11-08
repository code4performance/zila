create extension if not exists vector;
create database shop;
\connect shop
create extension if not exists vector;

create table customers (
  id int primary key,
  name text not null,
  email text null,
  city text null
);
insert into customers (id, name, email, city) values
  (1, 'Ana Souza', 'ana@example.com', 'Recife'),
  (2, 'João D''Arc', null, 'São Paulo'),
  (3, 'Zoë 日本', null, null),
  (4, 'Владимир Петров', 'vp@example.com', 'Москва');
insert into customers (id, name, email, city)
select n, 'Customer ' || n,
       case when n % 7 = 0 then null else 'c' || n || '@example.com' end,
       case when n % 5 = 0 then null else 'City ' || (n % 9) end
from generate_series(5, 60) as n;

create table orders (
  id int primary key,
  customer_id int not null references customers (id),
  total numeric(10,2) not null,
  placed date not null,
  note text null
);
insert into orders (id, customer_id, total, placed, note)
select (c - 1) * 3 + k, c, c * 10 + k * 1.25, date '2026-01-01' + (c + k),
       case when k = 3 then null else 'order ' || c || '-' || k end
from generate_series(1, 60) as c, generate_series(1, 3) as k;

create table items (
  id int primary key,
  label text not null,
  embedding vector(3) not null
);
insert into items values (1, 'a', '[1,0,0]'), (2, 'b', '[0,1,0]'), (3, 'c', '[0,0,1]');

create role app login password 'Brasa_app_dev_1';
create role reader login password 'Brasa_reader_dev_1';
grant all on all tables in schema public to app;
grant select on all tables in schema public to reader;
