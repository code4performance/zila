IF DB_ID('shop') IS NULL CREATE DATABASE shop;
GO
USE shop;
GO
IF OBJECT_ID('orders') IS NOT NULL DROP TABLE orders;
IF OBJECT_ID('customers') IS NOT NULL DROP TABLE customers;
GO
CREATE TABLE customers (
  id int NOT NULL PRIMARY KEY,
  name nvarchar(200) NOT NULL,
  email nvarchar(200) NULL,
  city nvarchar(200) NULL
);
INSERT INTO customers (id, name, email, city) VALUES
  (1, N'Ana Souza', N'ana@example.com', N'Recife'),
  (2, N'João D''Arc', NULL, N'São Paulo'),
  (3, N'Zoë 日本', NULL, NULL),
  (4, N'Владимир Петров', N'vp@example.com', N'Москва');
INSERT INTO customers (id, name, email, city)
SELECT n, N'Customer ' + CAST(n AS nvarchar(10)),
       CASE WHEN n % 7 = 0 THEN NULL ELSE N'c' + CAST(n AS nvarchar(10)) + N'@example.com' END,
       CASE WHEN n % 5 = 0 THEN NULL ELSE N'City ' + CAST(n % 9 AS nvarchar(10)) END
FROM (SELECT TOP (56) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) + 4 AS n FROM sys.all_objects) t;
CREATE TABLE orders (
  id int NOT NULL PRIMARY KEY,
  customer_id int NOT NULL REFERENCES customers (id),
  total decimal(10,2) NOT NULL,
  placed date NOT NULL,
  note nvarchar(200) NULL
);
INSERT INTO orders (id, customer_id, total, placed, note)
SELECT (c.n - 1) * 3 + k.n, c.n, c.n * 10 + k.n * 1.25, DATEADD(day, c.n + k.n, '2026-01-01'),
       CASE WHEN k.n = 3 THEN NULL ELSE N'order ' + CAST(c.n AS nvarchar(10)) + N'-' + CAST(k.n AS nvarchar(10)) END
FROM (SELECT TOP (60) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS n FROM sys.all_objects) c
CROSS JOIN (VALUES (1), (2), (3)) k(n);
GO
IF SUSER_ID('app') IS NULL CREATE LOGIN app WITH PASSWORD = 'Zila_app_dev_1', CHECK_POLICY = OFF;
IF SUSER_ID('reader') IS NULL CREATE LOGIN reader WITH PASSWORD = 'Zila_reader_dev_1', CHECK_POLICY = OFF;
IF USER_ID('app') IS NULL CREATE USER app FOR LOGIN app;
IF USER_ID('reader') IS NULL CREATE USER reader FOR LOGIN reader;
ALTER ROLE db_owner ADD MEMBER app;
ALTER ROLE db_datareader ADD MEMBER reader;
GO

-- A mirror of the `Users` table of the njai application database (same columns and keys), with
-- the login ai/ai, for the CRUD tests. It starts empty.
IF DB_ID('njai') IS NULL CREATE DATABASE njai;
GO
USE njai;
GO
IF OBJECT_ID('dbo.Users') IS NOT NULL DROP TABLE dbo.Users;
GO
CREATE TABLE dbo.Users (
  Id uniqueidentifier NOT NULL CONSTRAINT PK_Users PRIMARY KEY,
  Username nvarchar(256) NOT NULL,
  DisplayName nvarchar(256) NULL,
  Email nvarchar(256) NULL,
  Department nvarchar(256) NULL,
  AuthSource nvarchar(32) NOT NULL,
  Role nvarchar(32) NOT NULL,
  AccessStatus nvarchar(32) NOT NULL,
  LocalPasswordHash nvarchar(512) NULL,
  CreatedAtUtc datetime2 NOT NULL,
  PasswordChangedAtUtc datetime2 NULL,
  SuspendedAtUtc datetime2 NULL,
  SuspendedReason nvarchar(512) NULL,
  RevokedAtUtc datetime2 NULL,
  RevokedByUserId uniqueidentifier NULL
);
CREATE UNIQUE INDEX IX_Users_Username ON dbo.Users (Username);
GO
IF SUSER_ID('ai') IS NULL CREATE LOGIN ai WITH PASSWORD = 'ai', CHECK_POLICY = OFF;
IF USER_ID('ai') IS NULL CREATE USER ai FOR LOGIN ai;
ALTER ROLE db_owner ADD MEMBER ai;
GO
