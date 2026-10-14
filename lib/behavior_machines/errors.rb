# frozen_string_literal: true

module BehaviorMachines
  # Base class for errors raised by this library.
  class Error < StandardError; end

  # A node definition cannot be compiled into a tree.
  class BuildError < Error; end

  # A leaf's tick returned something other than :success, :failure, or :running.
  class InvalidStatusError < Error; end

  # A leaf called Tree#tick or Tree#halt on the tree that is ticking it.
  class ReentrantCallError < Error; end
end
