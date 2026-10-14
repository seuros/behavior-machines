# frozen_string_literal: true

require_relative 'test_helper'

class DecoratorTest < Minitest::Test
  MAX = BM::Definition::MAX_NANOS

  def test_status_decorators_rewrite_terminal_statuses
    decorators = {
      inverter: %i[failure success],
      force_success: %i[success success],
      force_failure: %i[failure failure]
    }
    decorators.each do |decorator, (on_success, on_failure)|
      { success: on_success, failure: on_failure, running: :running }.each do |child, expected|
        script = Script.new
        script.set('a', child)
        tree = tree(leaf('a').public_send(decorator))

        assert_equal expected, tree.tick(script, now: 0), "#{decorator}(#{child})"
        tree.halt(script)
        script.assert_idle!
      end
    end
  end

  def test_timeout_halts_its_child_at_the_deadline
    script = Script.new
    tree = tree(leaf('a').timeout(0.1))

    assert_equal :running, tree.tick(script, now: 0)
    assert_equal ms(100), tree.next_deadline
    assert_equal :running, tree.tick(script, now: ms(99))
    assert_equal :failure, tree.tick(script, now: ms(100))
    assert_equal [tick('a'), tick('a'), halted('a')], script.log
    assert_nil tree.next_deadline
    script.assert_idle!
  end

  def test_timeout_reports_a_child_that_finishes_in_time_and_restarts_its_deadline
    script = Script.new
    tree = tree(leaf('a').timeout(0.1))

    assert_equal :running, tree.tick(script, now: 0)
    script.set('a', :success)
    assert_equal :success, tree.tick(script, now: ms(50))
    script.set('a', :running)
    assert_equal :running, tree.tick(script, now: ms(500))
    assert_equal ms(600), tree.next_deadline
    tree.halt(script)
    script.assert_idle!
  end

  def test_zero_timeout_fails_without_ticking_its_child
    script = Script.new
    assert_equal :failure, tree(leaf('a').timeout(0)).tick(script, now: 0)
    assert_empty script.log
  end

  def test_sleep_succeeds_once_its_duration_has_passed
    tree = tree(BM.sleep(0.05))

    assert_equal :running, tree.tick(nil, now: ms(10))
    assert_equal ms(60), tree.next_deadline
    assert_equal :running, tree.tick(nil, now: ms(59))
    assert_equal :success, tree.tick(nil, now: ms(60))
    assert_equal :success, tree(BM.sleep(0)).tick(nil, now: 0)
  end

  def test_deadlines_past_the_end_of_the_clock_never_fire
    script = Script.new
    sleep_tree = tree(BM.sleep(Rational(2, 1_000_000_000)))
    assert_equal :running, sleep_tree.tick(script, now: MAX - 1)
    assert_nil sleep_tree.next_deadline
    assert_equal :running, sleep_tree.tick(script, now: MAX)

    exact = tree(BM.sleep(Rational(1, 1_000_000_000)))
    assert_equal :running, exact.tick(script, now: MAX - 1)
    assert_equal MAX, exact.next_deadline
    assert_equal :success, exact.tick(script, now: MAX)
  end

  def test_repeat_runs_its_child_once_per_tick_until_the_limit
    script = Script.new
    script.set('a', :success)
    tree = tree(leaf('a').repeat(3))

    assert_equal %i[running running success], Array.new(3) { tree.tick(script, now: 0) }
    assert_equal :running, tree.tick(script, now: 0)
    assert_equal 4, script.count(tick('a'))
  end

  def test_looping_decorators_stop_on_the_other_status
    cases = {
      repeat: [->(node) { node.repeat(:unlimited) }, :failure],
      retry_until_successful: [->(node) { node.retry_until_successful(:unlimited) }, :success],
      keep_running_until_failure: [->(node) { node.keep_running_until_failure }, :failure]
    }
    cases.each do |name, (wrap, stop)|
      script = Script.new
      script.set('a', stop == :success ? :failure : :success)
      tree = tree(wrap.call(leaf('a')))

      5.times { assert_equal :running, tree.tick(script, now: 0), name }
      script.set('a', stop)
      assert_equal stop, tree.tick(script, now: 0), name
      assert_equal 6, script.count(tick('a')), name
    end
  end

  def test_zero_limits_finish_without_ticking_the_child
    script = Script.new
    assert_equal :success, tree(leaf('a').repeat(0)).tick(script, now: 0)
    assert_equal :failure, tree(leaf('a').retry_until_successful(0)).tick(script, now: 0)
    assert_empty script.log
  end

  def test_halting_a_loop_halts_its_child_and_resets_the_count
    script = Script.new
    script.set('a', :success)
    tree = tree(BM.sequence(leaf('a'), leaf('b')).repeat(2))

    assert_equal :running, tree.tick(script, now: 0)
    tree.halt(script)
    assert_equal 1, script.count(halted('b'))
    script.set('b', :success)
    assert_equal %i[running success], Array.new(2) { tree.tick(script, now: 0) }
  end
end
