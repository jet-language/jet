stop("discovery executed source")
# fake <- function(input) input
text <- "also_fake <- function(input) input"
outer <- function(input) {
  nested <- function(input) input
  input
}

